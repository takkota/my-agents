use crate::domain::task::{Status, TaskLink};
use crate::storage::FsStore;
use std::process::Command;
use std::sync::mpsc;

pub struct PrMonitor {
    store: FsStore,
    /// Receives results from background PR check thread.
    result_rx: Option<mpsc::Receiver<Vec<PrMonitorEvent>>>,
}

#[derive(Debug, Clone)]
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
    if parts.len() < 5 {
        return None;
    }
    let len = parts.len();
    if parts[len - 2] == "pull" {
        let number = parts[len - 1].to_string();
        let repo = parts[len - 3].to_string();
        let owner = parts[len - 4].to_string();
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

/// A snapshot of task info needed for PR checking (Send-safe).
struct PrCheckTarget {
    task_id: String,
    project_id: String,
    prs: Vec<(String, String, String)>,
}

impl PrMonitor {
    pub fn new(store: FsStore) -> Self {
        Self {
            store,
            result_rx: None,
        }
    }

    /// Kick off a background thread to check PR statuses.
    /// Does nothing if a check is already in progress.
    pub fn start_check(&mut self) {
        if self.result_rx.is_some() {
            return; // check already in progress
        }

        // Collect targets from disk (fast, synchronous)
        let tasks = self.store.list_all_tasks().unwrap_or_default();
        let targets: Vec<PrCheckTarget> = tasks
            .iter()
            .filter(|t| matches!(t.status, Status::InProgress | Status::InReview))
            .filter_map(|t| {
                let prs = pr_links(&t.links);
                if prs.is_empty() {
                    None
                } else {
                    Some(PrCheckTarget {
                        task_id: t.id.clone(),
                        project_id: t.project_id.clone(),
                        prs,
                    })
                }
            })
            .collect();

        if targets.is_empty() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.result_rx = Some(rx);

        // Run gh calls in a background thread to avoid blocking the UI
        std::thread::spawn(move || {
            let mut events = Vec::new();
            for target in &targets {
                let all_merged = target.prs.iter().all(|(owner, repo, number)| {
                    is_pr_merged(owner, repo, number).unwrap_or(false)
                });
                if all_merged {
                    events.push(PrMonitorEvent::AllPrsMerged {
                        task_id: target.task_id.clone(),
                        project_id: target.project_id.clone(),
                    });
                }
            }
            let _ = tx.send(events);
        });
    }

    /// Poll for completed results. Returns events if the background check finished.
    pub fn poll_results(&mut self) -> Vec<PrMonitorEvent> {
        let rx = match &self.result_rx {
            Some(rx) => rx,
            None => return Vec::new(),
        };

        match rx.try_recv() {
            Ok(events) => {
                self.result_rx = None; // check finished, allow next one
                events
            }
            Err(mpsc::TryRecvError::Empty) => Vec::new(), // still running
            Err(mpsc::TryRecvError::Disconnected) => {
                self.result_rx = None; // thread panicked or dropped
                Vec::new()
            }
        }
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
