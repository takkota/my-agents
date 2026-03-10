use crate::domain::task::{Status, TaskLink};
use crate::storage::FsStore;
use std::process::Command;

pub struct PrMonitor {
    store: FsStore,
}

pub enum PrMonitorEvent {
    /// All PRs for this task have been merged
    AllPrsMerged {
        task_id: String,
        project_id: String,
    },
}

/// Parse owner, repo, and PR number from a GitHub PR URL.
/// e.g. "https://github.com/owner/repo/pull/123" -> Some(("owner", "repo", "123"))
fn parse_github_pr(url: &str) -> Option<(String, String, String)> {
    let url = url.trim_end_matches('/');
    let parts: Vec<&str> = url.split('/').collect();
    // Expected: [..., "github.com", owner, repo, "pull", number]
    if parts.len() < 5 {
        return None;
    }
    let len = parts.len();
    if parts[len - 2] == "pull" {
        let number = parts[len - 1].to_string();
        let repo = parts[len - 3].to_string();
        let owner = parts[len - 4].to_string();
        // Validate that number is numeric
        if number.chars().all(|c| c.is_ascii_digit()) {
            return Some((owner, repo, number));
        }
    }
    None
}

/// Check if a PR is merged using the `gh` CLI.
fn is_pr_merged(owner: &str, repo: &str, number: &str) -> Option<bool> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            number,
            "--repo",
            &format!("{}/{}", owner, repo),
            "--json",
            "state",
            "-q",
            ".state",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(state == "MERGED")
}

/// Extract all GitHub PR links from a task's links.
fn pr_links(links: &[TaskLink]) -> Vec<(String, String, String)> {
    links
        .iter()
        .filter_map(|link| parse_github_pr(&link.url))
        .collect()
}

impl PrMonitor {
    pub fn new(store: FsStore) -> Self {
        Self { store }
    }

    /// Check all InReview/InProgress tasks that have PR links.
    /// Returns events for tasks where all PRs are merged.
    pub fn check_all(&self) -> Vec<PrMonitorEvent> {
        let mut events = Vec::new();
        let tasks = self.store.list_all_tasks().unwrap_or_default();

        for task in &tasks {
            if !matches!(task.status, Status::InProgress | Status::InReview) {
                continue;
            }

            let prs = pr_links(&task.links);
            if prs.is_empty() {
                continue;
            }

            let all_merged = prs.iter().all(|(owner, repo, number)| {
                is_pr_merged(owner, repo, number).unwrap_or(false)
            });

            if all_merged {
                events.push(PrMonitorEvent::AllPrsMerged {
                    task_id: task.id.clone(),
                    project_id: task.project_id.clone(),
                });
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_pr() {
        let result = parse_github_pr("https://github.com/owner/repo/pull/123");
        assert_eq!(
            result,
            Some(("owner".into(), "repo".into(), "123".into()))
        );

        let result = parse_github_pr("https://github.com/owner/repo/pull/123/");
        assert_eq!(
            result,
            Some(("owner".into(), "repo".into(), "123".into()))
        );

        assert!(parse_github_pr("https://github.com/owner/repo/issues/123").is_none());
        assert!(parse_github_pr("https://example.com").is_none());
    }
}
