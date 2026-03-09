use crate::domain::task::AgentCli;
use crate::error::AppResult;
use std::path::Path;
use std::process::Command;

pub struct TmuxService;

impl TmuxService {
    pub fn new() -> Self {
        Self
    }

    fn tmux_cmd() -> Command {
        Command::new("tmux")
    }

    pub fn is_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn session_exists(&self, name: &str) -> bool {
        Self::tmux_cmd()
            .args(["has-session", "-t", name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn create_session(&self, name: &str, start_dir: &Path) -> AppResult<()> {
        let status = Self::tmux_cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                name,
                "-c",
                &start_dir.to_string_lossy(),
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!("Failed to create tmux session: {}", name);
        }

        // Bind Ctrl+Q to detach
        Self::tmux_cmd()
            .args([
                "bind-key",
                "-t",
                name,
                "-T",
                "root",
                "C-q",
                "detach-client",
            ])
            .output()
            .ok();

        Ok(())
    }

    pub fn attach_session(&self, name: &str) -> AppResult<()> {
        let status = Self::tmux_cmd()
            .args(["attach-session", "-t", name])
            .status()?;
        if !status.success() {
            anyhow::bail!("Failed to attach tmux session: {}", name);
        }
        Ok(())
    }

    pub fn kill_session(&self, name: &str) -> AppResult<()> {
        Self::tmux_cmd()
            .args(["kill-session", "-t", name])
            .output()?;
        Ok(())
    }

    pub fn capture_pane(&self, session: &str) -> AppResult<String> {
        let output = Self::tmux_cmd()
            .args(["capture-pane", "-t", session, "-p"])
            .output()?;
        if !output.status.success() {
            anyhow::bail!("Failed to capture tmux pane for session: {}", session);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn launch_agent(&self, session: &str, cli: &AgentCli) -> AppResult<()> {
        if let Some(cmd) = cli.command() {
            Self::tmux_cmd()
                .args(["send-keys", "-t", session, cmd, "Enter"])
                .output()?;
        }
        Ok(())
    }

    pub fn session_name(project_id: &str, task_id: &str) -> String {
        format!("ma-{}-{}", project_id, &task_id[..task_id.len().min(6)])
    }

    pub fn list_sessions(&self) -> AppResult<Vec<String>> {
        let output = Self::tmux_cmd()
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()?;
        if !output.status.success() {
            return Ok(vec![]);
        }
        let sessions = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
        Ok(sessions)
    }
}
