use crate::domain::project::{Project, RepoRef};
use fs2::FileExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

/// Label applied to an issue once the monitor has claimed it (created a task).
/// Claimed issues are excluded from subsequent scans.
pub const CLAIM_LABEL: &str = "on my-task";

/// A candidate project scan: which repos to check and the matching config.
#[derive(Debug, Clone)]
struct ScanProject {
    project_id: String,
    repos: Vec<RepoRef>,
    labels: Vec<String>,
}

/// An issue that has been successfully claimed and should become a task.
#[derive(Debug, Clone)]
pub struct IssueMonitorEvent {
    pub project_id: String,
    /// `owner/repo` the issue belongs to (kept for diagnostics/logging).
    #[allow(dead_code)]
    pub repo_full: String,
    pub number: u64,
    pub title: String,
    pub url: String,
}

pub struct IssueMonitor {
    /// Receives results from the background scan thread.
    result_rx: Option<mpsc::Receiver<Vec<IssueMonitorEvent>>>,
}

impl IssueMonitor {
    pub fn new() -> Self {
        Self { result_rx: None }
    }

    /// Kick off a background scan. Does nothing if a scan is already in flight,
    /// if the cross-process lock cannot be acquired (another instance is
    /// scanning), or if there are no enabled projects to scan.
    ///
    /// `lock_path` is an advisory flock file (e.g. `~/.my-agents/issue_monitor.lock`)
    /// held for the duration of the scan so only one process scans at a time.
    /// The lock is auto-released when the owning process exits.
    pub fn start_check(&mut self, projects: Vec<Project>, lock_path: PathBuf) {
        if self.result_rx.is_some() {
            return; // scan already in progress
        }

        let scan_projects: Vec<ScanProject> = projects
            .iter()
            .filter(|p| p.issue_monitor_enabled)
            .map(|p| {
                // Resolve monitored repos by name; fall back to all repos if
                // none were explicitly configured.
                let repos: Vec<RepoRef> = if p.issue_monitor_repos.is_empty() {
                    p.repos.clone()
                } else {
                    p.repos
                        .iter()
                        .filter(|r| p.issue_monitor_repos.contains(&r.name))
                        .cloned()
                        .collect()
                };
                ScanProject {
                    project_id: p.id.clone(),
                    repos,
                    labels: p.issue_monitor_labels.clone(),
                }
            })
            .filter(|sp| !sp.repos.is_empty())
            .collect();

        if scan_projects.is_empty() {
            return;
        }

        // Acquire the cross-process lock. If another process holds it, skip
        // this cycle entirely — it is already handling all candidate issues.
        let lock_file = match std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&lock_path)
        {
            Ok(f) => f,
            Err(_) => return,
        };
        // try_lock_exclusive returns Err if already locked by another process.
        if lock_file.try_lock_exclusive().is_err() {
            return;
        }
        // `lock_file` is moved into the thread to keep the lock held until the
        // scan completes, then dropped to release it.

        let (tx, rx) = mpsc::channel();
        self.result_rx = Some(rx);

        std::thread::spawn(move || {
            // `_lock_file` keeps the advisory lock alive for the whole scan.
            let _lock_file = lock_file;
            let mut events = Vec::new();
            for sp in &scan_projects {
                for repo in &sp.repos {
                    let owner_repo = match resolve_owner_repo(&repo.path) {
                        Some(or) => or,
                        None => continue, // not a GitHub repo / no origin
                    };

                    // Ensure the claim label exists (best-effort, ignore errors).
                    ensure_label(&owner_repo);

                    let issues = match list_issues(&owner_repo, &sp.labels) {
                        Ok(issues) => issues,
                        Err(_) => continue,
                    };

                    for issue in issues {
                        // Skip already-claimed issues (defensive: the gh search
                        // should already exclude them, but enforce client-side).
                        if issue
                            .labels
                            .iter()
                            .any(|l| l.eq_ignore_ascii_case(CLAIM_LABEL))
                        {
                            continue;
                        }
                        // Claim the issue by adding the label. Only emit an
                        // event for issues we successfully claimed.
                        if add_label(&owner_repo, issue.number).is_ok() {
                            events.push(IssueMonitorEvent {
                                project_id: sp.project_id.clone(),
                                repo_full: owner_repo.clone(),
                                number: issue.number,
                                title: issue.title,
                                url: issue.url,
                            });
                        }
                    }
                }
            }
            let _ = tx.send(events);
        });
    }

    /// Poll for completed scan results. Returns events if the background scan
    /// finished.
    pub fn poll_results(&mut self) -> Vec<IssueMonitorEvent> {
        let rx = match &self.result_rx {
            Some(rx) => rx,
            None => return Vec::new(),
        };

        match rx.try_recv() {
            Ok(events) => {
                self.result_rx = None;
                events
            }
            Err(mpsc::TryRecvError::Empty) => Vec::new(),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.result_rx = None;
                Vec::new()
            }
        }
    }
}

impl Default for IssueMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct Issue {
    number: u64,
    title: String,
    url: String,
    labels: Vec<String>,
}

/// Resolve a local git working copy to a GitHub `owner/repo` string by reading
/// `remote.origin.url`. Supports HTTPS and SSH URLs.
fn resolve_owner_repo(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo_path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_github_url(&url)
}

/// Extract `owner/repo` from a GitHub remote URL.
/// Supports:
///   - https://github.com/owner/repo(.git)
///   - git@github.com:owner/repo(.git)
///   - ssh://git@github.com/owner/repo(.git)
fn parse_github_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    // ssh form: git@github.com:owner/repo
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return normalize_owner_repo(rest);
    }
    // ssh:// or https:// form
    let rest = url
        .strip_prefix("ssh://git@github.com/")
        .or_else(|| url.strip_prefix("ssh://github.com/"))
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    normalize_owner_repo(rest)
}

fn normalize_owner_repo(rest: &str) -> Option<String> {
    let rest = rest.trim_end_matches('/');
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    let owner = parts[parts.len() - 2];
    let repo = parts[parts.len() - 1];
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{}/{}", owner, repo))
}

/// Create the claim label if it doesn't exist. Errors are ignored (e.g. label
/// already exists, or insufficient permissions — the subsequent `--add-label`
/// call will surface a real failure).
fn ensure_label(owner_repo: &str) {
    let _ = Command::new("gh")
        .args(["label", "create", CLAIM_LABEL])
        .args(["--repo", owner_repo])
        .args(["--description", "Picked up by my-agents issue monitor"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[derive(serde::Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    url: String,
    labels: Vec<GhLabel>,
}

#[derive(serde::Deserialize)]
struct GhLabel {
    name: String,
}

/// List open issues in the repo that have ALL of the given labels (AND).
/// When no labels are specified, all open issues are returned.
/// Already-claimed issues (with `on my-task`) are excluded via `--search`.
fn list_issues(owner_repo: &str, labels: &[String]) -> anyhow::Result<Vec<Issue>> {
    let mut args = vec![
        "issue".to_string(),
        "list".to_string(),
        "--repo".to_string(),
        owner_repo.to_string(),
        "--state".to_string(),
        "open".to_string(),
        "--json".to_string(),
        "number,title,url,labels".to_string(),
        "--limit".to_string(),
        "100".to_string(),
    ];
    for label in labels {
        args.push("--label".to_string());
        args.push(label.clone());
    }
    // Exclude already-claimed issues.
    args.push("--search".to_string());
    args.push(format!("-label:\"{}\"", CLAIM_LABEL));

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue list failed: {}", stderr.trim());
    }
    let gh_issues: Vec<GhIssue> = serde_json::from_slice(&output.stdout)?;
    Ok(gh_issues
        .into_iter()
        .map(|i| Issue {
            number: i.number,
            title: i.title,
            url: i.url,
            labels: i.labels.into_iter().map(|l| l.name).collect(),
        })
        .collect())
}

/// Add the claim label to an issue. Returns Err on failure.
fn add_label(owner_repo: &str, number: u64) -> anyhow::Result<()> {
    let output = Command::new("gh")
        .args([
            "issue",
            "edit",
            &number.to_string(),
            "--repo",
            owner_repo,
            "--add-label",
            CLAIM_LABEL,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue edit failed: {}", stderr.trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url_https() {
        assert_eq!(
            parse_github_url("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            parse_github_url("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_url_ssh() {
        assert_eq!(
            parse_github_url("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            parse_github_url("git@github.com:owner/repo"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            parse_github_url("ssh://git@github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_url_invalid() {
        assert_eq!(parse_github_url("https://gitlab.com/owner/repo"), None);
        assert_eq!(parse_github_url("not a url"), None);
        assert_eq!(parse_github_url(""), None);
    }

    #[test]
    fn test_normalize_owner_repo() {
        assert_eq!(
            normalize_owner_repo("owner/repo"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            normalize_owner_repo("owner/repo/"),
            Some("owner/repo".to_string())
        );
        assert_eq!(normalize_owner_repo("owner"), None);
    }
}
