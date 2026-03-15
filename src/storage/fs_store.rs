use crate::config::Config;
use crate::domain::project::Project;
use crate::domain::task::Task;
use crate::error::AppResult;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Remove a directory tree, stripping macOS ACLs first if plain removal fails.
///
/// Sandboxed agent processes (e.g. Claude Code) may add `deny delete` ACLs to
/// files they create.  `fs::remove_dir_all` cannot delete such entries, so we
/// fall back to `chmod -RN` (which clears all ACLs) and retry.
fn force_remove_dir_all(dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(_first_err) => {
            // Strip ACLs and retry (macOS only; on other platforms this is a
            // no-op because chmod -RN is a macOS extension).
            let _ = std::process::Command::new("chmod")
                .args(["-RN"])
                .arg(dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            fs::remove_dir_all(dir)
                .with_context(|| format!("Failed to remove directory: {}", dir.display()))?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct FsStore {
    projects_dir: PathBuf,
    bin_dir: PathBuf,
}

impl FsStore {
    pub fn new(config: &Config) -> AppResult<Self> {
        let projects_dir = config.projects_dir();
        let bin_dir = config.data_dir.join("bin");
        fs::create_dir_all(&projects_dir)?;
        let store = Self { projects_dir, bin_dir };
        store.install_scripts(&config.data_dir)?;
        Ok(store)
    }

    /// Install bundled scripts to `<data_dir>/bin/`.
    fn install_scripts(&self, data_dir: &std::path::Path) -> AppResult<()> {
        let bin_dir = data_dir.join("bin");
        fs::create_dir_all(&bin_dir)?;

        let scripts: &[(&str, &str)] = &[
            ("ma-task", include_str!("../../scripts/ma-task")),
            ("ma-codex-notify", include_str!("../../scripts/ma-codex-notify")),
        ];

        for (name, content) in scripts {
            let dest = bin_dir.join(name);
            let needs_update = if dest.exists() {
                fs::read_to_string(&dest).map_or(true, |existing| &existing != content)
            } else {
                true
            };
            if needs_update {
                fs::write(&dest, content)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    // 0o755: owner rwx, group/other rx — required for executable scripts
                    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
                }
            }
        }

        Ok(())
    }

    pub fn project_dir(&self, project_id: &str) -> PathBuf {
        assert!(Self::is_safe_id(project_id), "Invalid project_id: contains path traversal characters");
        self.projects_dir.join(project_id)
    }

    pub fn tasks_dir(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("tasks")
    }

    pub fn task_dir(&self, project_id: &str, task_id: &str) -> PathBuf {
        assert!(Self::is_safe_id(task_id), "Invalid task_id: contains path traversal characters");
        self.tasks_dir(project_id).join(task_id)
    }

    fn is_safe_id(id: &str) -> bool {
        !id.is_empty()
            && !id.contains('/')
            && !id.contains('\\')
            && !id.contains('\0')
            && id != "."
            && id != ".."
            && !id.contains("..")
    }

    /// Compute a lightweight fingerprint of all task.json files.
    /// Returns the maximum mtime (as duration since UNIX_EPOCH in millis)
    /// combined with the total count of task files. Changes to either
    /// value indicate external modifications.
    pub fn data_fingerprint(&self) -> (u128, usize) {
        let mut max_mtime: u128 = 0;
        let mut count: usize = 0;

        let Ok(projects) = fs::read_dir(&self.projects_dir) else {
            return (0, 0);
        };
        for pentry in projects.flatten() {
            let tasks_dir = pentry.path().join("tasks");
            let Ok(tasks) = fs::read_dir(&tasks_dir) else {
                continue;
            };
            for tentry in tasks.flatten() {
                let json_path = tentry.path().join("task.json");
                if let Ok(meta) = fs::metadata(&json_path) {
                    count += 1;
                    if let Ok(mtime) = meta.modified() {
                        let millis = mtime
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        if millis > max_mtime {
                            max_mtime = millis;
                        }
                    }
                }
            }
        }
        (max_mtime, count)
    }

    // Project operations

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        let mut projects = Vec::new();
        if !self.projects_dir.exists() {
            return Ok(projects);
        }
        for entry in fs::read_dir(&self.projects_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let json_path = entry.path().join("project.json");
                if json_path.exists() {
                    let content = fs::read_to_string(&json_path)
                        .with_context(|| format!("reading {:?}", json_path))?;
                    let project: Project = serde_json::from_str(&content)
                        .with_context(|| format!("parsing {:?}", json_path))?;
                    projects.push(project);
                }
            }
        }
        projects.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(projects)
    }

    pub fn save_project(&self, project: &Project) -> AppResult<()> {
        let dir = self.project_dir(&project.id);
        fs::create_dir_all(&dir)?;
        fs::create_dir_all(dir.join("tasks"))?;
        let json_path = dir.join("project.json");
        let content = serde_json::to_string_pretty(project)?;
        fs::write(json_path, content)?;
        Ok(())
    }

    pub fn delete_project(&self, project_id: &str) -> AppResult<()> {
        let dir = self.project_dir(project_id);
        force_remove_dir_all(&dir)
    }

    // Task operations

    pub fn list_tasks(&self, project_id: &str) -> AppResult<Vec<Task>> {
        let mut tasks = Vec::new();
        let tasks_dir = self.tasks_dir(project_id);
        if !tasks_dir.exists() {
            return Ok(tasks);
        }
        for entry in fs::read_dir(&tasks_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let json_path = entry.path().join("task.json");
                if json_path.exists() {
                    let content = fs::read_to_string(&json_path)
                        .with_context(|| format!("reading {:?}", json_path))?;
                    let task: Task = serde_json::from_str(&content)
                        .with_context(|| format!("parsing {:?}", json_path))?;
                    tasks.push(task);
                }
            }
        }
        tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(tasks)
    }

    pub fn list_all_tasks(&self) -> AppResult<Vec<Task>> {
        let mut all_tasks = Vec::new();
        let projects = self.list_projects()?;
        for project in &projects {
            let tasks = self.list_tasks(&project.id)?;
            all_tasks.extend(tasks);
        }
        Ok(all_tasks)
    }

    pub fn save_task(&self, task: &Task) -> AppResult<()> {
        let dir = self.task_dir(&task.project_id, &task.id);
        fs::create_dir_all(&dir)?;
        let json_path = dir.join("task.json");
        let content = serde_json::to_string_pretty(task)?;
        fs::write(json_path, content)?;
        Ok(())
    }

    pub fn delete_task_dir(&self, project_id: &str, task_id: &str) -> AppResult<()> {
        let dir = self.task_dir(project_id, task_id);
        force_remove_dir_all(&dir)
    }

    pub fn write_agent_config_files(&self, task: &Task, pr_prompt: &str, project: Option<&Project>) -> AppResult<()> {
        let dir = self.task_dir(&task.project_id, &task.id);

        // Write CLAUDE.md with references and skill trigger
        let mut claude_lines: Vec<String> = task
            .worktrees
            .iter()
            .filter_map(|wt| {
                let claude_md = wt.upstream_path.join("CLAUDE.md");
                if claude_md.exists() {
                    Some(format!("@{}/CLAUDE.md", wt.repo_name))
                } else {
                    None
                }
            })
            .collect();
        if task.agent_cli == crate::domain::task::AgentCli::Claude {
            claude_lines.push(String::new());
            claude_lines.push("## Task Management".to_string());
            claude_lines.push(format!(
                "This session is managed by my-agents. Task ID: `{}`, Project: `{}`.",
                task.id, task.project_id
            ));
            claude_lines.push(
                "Use the `/task-management` skill when you need to check task details, \
                 update status, add links, or create new tasks."
                    .to_string(),
            );
            if !pr_prompt.trim().is_empty() {
                claude_lines.push(String::new());
                claude_lines.push("## Pull Request".to_string());
                claude_lines.push(pr_prompt.to_string());
            }
        }
        if !claude_lines.is_empty() {
            fs::write(dir.join("CLAUDE.md"), claude_lines.join("\n") + "\n")?;
        }

        // Write GEMINI.md with references and skill trigger
        let mut gemini_lines: Vec<String> = task
            .worktrees
            .iter()
            .filter_map(|wt| {
                let gemini_md = wt.upstream_path.join("GEMINI.md");
                if gemini_md.exists() {
                    Some(format!("@{}/GEMINI.md", wt.repo_name))
                } else {
                    None
                }
            })
            .collect();
        if task.agent_cli == crate::domain::task::AgentCli::Gemini {
            gemini_lines.push(String::new());
            gemini_lines.push("## Task Management".to_string());
            gemini_lines.push(format!(
                "This session is managed by my-agents. Task ID: `{}`, Project: `{}`.",
                task.id, task.project_id
            ));
            gemini_lines.push(
                "Use the task-management skill when you need to check task details, \
                 update status, add links, or create new tasks."
                    .to_string(),
            );
            if !pr_prompt.trim().is_empty() {
                gemini_lines.push(String::new());
                gemini_lines.push("## Pull Request".to_string());
                gemini_lines.push(pr_prompt.to_string());
            }
        }
        if !gemini_lines.is_empty() {
            fs::write(dir.join("GEMINI.md"), gemini_lines.join("\n") + "\n")?;
        }

        // Write AGENTS.md with references and skill trigger
        let mut agents_lines: Vec<String> = task
            .worktrees
            .iter()
            .filter_map(|wt| {
                let agents_md = wt.upstream_path.join("AGENTS.md");
                if agents_md.exists() {
                    Some(format!("@{}/AGENTS.md", wt.repo_name))
                } else {
                    None
                }
            })
            .collect();
        if task.agent_cli == crate::domain::task::AgentCli::Codex {
            agents_lines.push(String::new());
            agents_lines.push("## Task Management".to_string());
            agents_lines.push(format!(
                "This session is managed by my-agents. Task ID: `{}`, Project: `{}`.",
                task.id, task.project_id
            ));
            agents_lines.push(
                "Use the `$task-management` skill when you need to check task details, \
                 update status, add links, or create new tasks."
                    .to_string(),
            );
            if !pr_prompt.trim().is_empty() {
                agents_lines.push(String::new());
                agents_lines.push("## Pull Request".to_string());
                agents_lines.push(pr_prompt.to_string());
            }
        }
        if !agents_lines.is_empty() {
            fs::write(dir.join("AGENTS.md"), agents_lines.join("\n") + "\n")?;
        }

        // Write Claude Code hooks config and skill for Claude agent tasks
        if task.agent_cli == crate::domain::task::AgentCli::Claude {
            self.write_claude_hooks(task)?;
            self.copy_claude_settings_local(task)?;
            self.write_claude_skill(task)?;
        }

        // Write Codex skill and configure notify for Codex agent tasks
        if task.agent_cli == crate::domain::task::AgentCli::Codex {
            self.write_codex_skill(task)?;
            self.write_codex_notify(task)?;
        }

        // Write Gemini CLI hooks config and skill for Gemini agent tasks
        if task.agent_cli == crate::domain::task::AgentCli::Gemini {
            self.write_gemini_hooks(task)?;
            self.write_gemini_skill(task)?;
        }

        // Write dev-environment skill if project has dev_environment_prompt
        let dev_env_prompt = project.and_then(|p| p.dev_environment_prompt.as_deref());
        if let Some(prompt) = dev_env_prompt {
            if task.agent_cli == crate::domain::task::AgentCli::Claude {
                Self::write_dev_env_skill_claude(&dir, task, prompt)?;
                // Append dev-environment skill reference to CLAUDE.md
                let claude_md_path = dir.join("CLAUDE.md");
                let mut content = if claude_md_path.exists() {
                    fs::read_to_string(&claude_md_path)?
                } else {
                    String::new()
                };
                content.push_str("\n## Dev Environment\nUse the `/dev-environment` skill to start the development server and register preview URLs.\n");
                fs::write(&claude_md_path, content)?;
            }
            if task.agent_cli == crate::domain::task::AgentCli::Codex {
                Self::write_dev_env_skill_codex(&dir, task, prompt)?;
                // Append dev-environment skill reference to AGENTS.md
                let agents_md_path = dir.join("AGENTS.md");
                let mut content = if agents_md_path.exists() {
                    fs::read_to_string(&agents_md_path)?
                } else {
                    String::new()
                };
                content.push_str("\n## Dev Environment\nUse the `$dev-environment` skill to start the development server and register preview URLs.\n");
                fs::write(&agents_md_path, content)?;
            }
            if task.agent_cli == crate::domain::task::AgentCli::Gemini {
                Self::write_dev_env_skill_gemini(&dir, task, prompt)?;
                // Append dev-environment skill reference to GEMINI.md
                let gemini_md_path = dir.join("GEMINI.md");
                let mut content = if gemini_md_path.exists() {
                    fs::read_to_string(&gemini_md_path)?
                } else {
                    String::new()
                };
                content.push_str("\n## Dev Environment\nUse the dev-environment skill to start the development server and register preview URLs.\n");
                fs::write(&gemini_md_path, content)?;
            }
        }

        // Copy project-level skills to task directory so they are discoverable
        self.copy_project_skills(task)?;

        // Mark the task directory as trusted in Claude Code's config
        // to bypass the "Quick safety check" workspace trust dialog.
        // Claude Code checks parent directories, so this covers the task dir.
        Self::ensure_claude_trust(&dir)?;

        Ok(())
    }

    /// Register a directory as trusted in `~/.claude.json` so Claude Code
    /// skips the workspace trust dialog. Claude walks parent directories
    /// when checking trust, so trusting a parent covers all children.
    fn ensure_claude_trust(dir: &std::path::Path) -> AppResult<()> {
        let claude_json_path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
            .join(".claude.json");

        let mut config: serde_json::Value = if claude_json_path.exists() {
            let content = fs::read_to_string(&claude_json_path)?;
            serde_json::from_str(&content)?
        } else {
            serde_json::json!({})
        };

        let dir_str = dir.to_string_lossy().to_string();
        let projects = config
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("~/.claude.json is not an object"))?
            .entry("projects")
            .or_insert_with(|| serde_json::json!({}));

        let project_entry = projects
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("projects is not an object"))?
            .entry(&dir_str)
            .or_insert_with(|| serde_json::json!({}));

        if project_entry.get("hasTrustDialogAccepted") == Some(&serde_json::json!(true)) {
            return Ok(());
        }

        project_entry
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("project entry is not an object"))?
            .insert(
                "hasTrustDialogAccepted".to_string(),
                serde_json::json!(true),
            );

        fs::write(
            &claude_json_path,
            serde_json::to_string_pretty(&config)?,
        )?;

        Ok(())
    }

    /// Remove a task directory (and its worktree sub-directories) from the
    /// `projects` map in `~/.claude.json` so the file doesn't grow unboundedly.
    pub fn remove_claude_trust(&self, task: &Task) -> AppResult<()> {
        let claude_json_path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
            .join(".claude.json");

        if !claude_json_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&claude_json_path)?;
        let mut config: serde_json::Value = serde_json::from_str(&content)?;

        let projects = match config
            .as_object_mut()
            .and_then(|o| o.get_mut("projects"))
            .and_then(|v| v.as_object_mut())
        {
            Some(p) => p,
            None => return Ok(()),
        };

        // Collect paths to remove: the task directory itself + any worktree sub-paths
        let task_dir_path = self.task_dir(&task.project_id, &task.id);
        let mut to_remove: Vec<String> = Vec::new();
        for key in projects.keys() {
            if Path::new(key).starts_with(&task_dir_path) {
                to_remove.push(key.clone());
            }
        }

        if to_remove.is_empty() {
            return Ok(());
        }

        for key in &to_remove {
            projects.remove(key);
        }

        fs::write(
            &claude_json_path,
            serde_json::to_string_pretty(&config)?,
        )?;

        Ok(())
    }

    /// Copy skills from the project directory to the task directory.
    /// Claude Code and Codex only discover skills in/below the CWD, so
    /// project-level skills must be copied (symlinked) into each task dir.
    fn copy_project_skills(&self, task: &Task) -> AppResult<()> {
        let project_dir = self.project_dir(&task.project_id);
        let task_dir = self.task_dir(&task.project_id, &task.id);

        // Claude Code skills: .claude/skills/
        let project_claude_skills = project_dir.join(".claude").join("skills");
        if project_claude_skills.is_dir() {
            let task_claude_skills = task_dir.join(".claude").join("skills");
            fs::create_dir_all(&task_claude_skills)?;
            Self::copy_skills_dir(&project_claude_skills, &task_claude_skills)?;
        }

        // Codex skills: .agents/skills/
        let project_agents_skills = project_dir.join(".agents").join("skills");
        if project_agents_skills.is_dir() {
            let task_agents_skills = task_dir.join(".agents").join("skills");
            fs::create_dir_all(&task_agents_skills)?;
            Self::copy_skills_dir(&project_agents_skills, &task_agents_skills)?;
        }

        // Gemini CLI skills: .gemini/skills/
        let project_gemini_skills = project_dir.join(".gemini").join("skills");
        if project_gemini_skills.is_dir() {
            let task_gemini_skills = task_dir.join(".gemini").join("skills");
            fs::create_dir_all(&task_gemini_skills)?;
            Self::copy_skills_dir(&project_gemini_skills, &task_gemini_skills)?;
        }

        Ok(())
    }

    /// Copy skill subdirectories from `src` into `dst`.
    /// Each child directory in `src` is symlinked into `dst` unless a
    /// directory with the same name already exists (e.g. task-management).
    /// PM-only skills (e.g. `pm-manager`) are excluded to prevent them from
    /// leaking into regular task sessions.
    /// Errors on individual entries are logged and skipped rather than
    /// aborting the entire operation.
    const PM_ONLY_SKILLS: &[&str] = &["pm-manager"];

    fn copy_skills_dir(src: &std::path::Path, dst: &std::path::Path) -> AppResult<()> {
        for entry in fs::read_dir(src)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name();

            // Skip PM-only skills to prevent them from appearing in regular tasks.
            if let Some(name_str) = name.to_str() {
                if Self::PM_ONLY_SKILLS.contains(&name_str) {
                    continue;
                }
            }
            let dest = dst.join(&name);

            // Skip if the task already has this skill (e.g. built-in task-management).
            // Use symlink_metadata to detect broken symlinks as well.
            if fs::symlink_metadata(&dest).is_ok() {
                continue;
            }

            let src_path = entry.path();

            // Only process directories (or symlinks pointing to directories).
            // Skip plain files or other entry types.
            let meta = match fs::metadata(&src_path) {
                Ok(m) => m,
                Err(_) => continue, // broken symlink or inaccessible — skip
            };
            if !meta.is_dir() {
                continue;
            }

            // Resolve to an absolute canonical path so the symlink works
            // regardless of the relative position of the task directory.
            let canonical = match fs::canonicalize(&src_path) {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Create a symlink in the task dir pointing to the canonical source
            // Silently ignore symlink failures (e.g. already exists) to avoid
            // writing to stderr which corrupts the TUI display.
            let _ = std::os::unix::fs::symlink(&canonical, &dest);
        }
        Ok(())
    }

    /// Write `.codex/config.toml` in the task directory with `notify`
    /// configured for automatic status tracking via `ma-codex-notify`.
    /// Project-level config takes precedence over `~/.codex/config.toml`.
    fn write_codex_notify(&self, task: &Task) -> AppResult<()> {
        let task_dir = self.task_dir(&task.project_id, &task.id);
        let codex_dir = task_dir.join(".codex");
        fs::create_dir_all(&codex_dir)?;

        let script_path = self.bin_dir.join("ma-codex-notify");
        let script_path_str = script_path.to_string_lossy();

        let content = format!(
            "# Auto-generated by my-agents for task status tracking\nnotify = [\"{}\"]\n",
            script_path_str
        );

        fs::write(codex_dir.join("config.toml"), content)?;
        Ok(())
    }

    /// Write `.claude/settings.json` in the task directory with hooks that
    /// support task management (prompt activity detection, agent stop detection,
    /// PR link discovery). Also merges non-hook settings (e.g. `enabledPlugins`)
    /// from the project-level `.claude/settings.json` if present.
    pub fn write_claude_hooks(&self, task: &Task) -> AppResult<()> {
        let task_dir = self.task_dir(&task.project_id, &task.id);

        let claude_dir = task_dir.join(".claude");
        fs::create_dir_all(&claude_dir)?;

        let pr_links_path = task_dir.join(".pr_links");
        let pr_links_path_str = pr_links_path.to_string_lossy();
        let prompt_submitted_path = task_dir.join(".prompt_submitted");
        let prompt_submitted_path_str = prompt_submitted_path.to_string_lossy();
        let agent_stopped_path = task_dir.join(".agent_stopped");
        let agent_stopped_path_str = agent_stopped_path.to_string_lossy();

        let mut settings = serde_json::json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!(
                                    "touch {} && rm -f {}",
                                    shell_escape(&prompt_submitted_path_str),
                                    shell_escape(&agent_stopped_path_str)
                                )
                            }
                        ]
                    }
                ],
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!(
                                    "touch {}",
                                    shell_escape(&agent_stopped_path_str)
                                )
                            }
                        ]
                    }
                ],
                "PostToolUse": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!(
                                    "grep -oE 'https://github\\.com/[^\"/]+/[^\"/]+/pull/[0-9]+' | grep -vE '/(owner|org|example|user|your-org)/(repo|repository|my-repo|your-repo|example)/' >> {} || true",
                                    shell_escape(&pr_links_path_str)
                                )
                            }
                        ]
                    }
                ]
            }
        });

        // Merge allowed settings from project-level .claude/settings.json
        // (e.g. enabledPlugins) so that project configuration is inherited.
        // Uses an allowlist to avoid leaking unknown/dangerous keys.
        const ALLOWED_PROJECT_KEYS: &[&str] = &["enabledPlugins"];

        let project_settings_path = self
            .project_dir(&task.project_id)
            .join(".claude")
            .join("settings.json");
        if project_settings_path.exists() {
            match fs::read_to_string(&project_settings_path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(project_settings) => {
                        if let Some(project_obj) = project_settings.as_object() {
                            let Some(settings_obj) = settings.as_object_mut() else {
                                // settings should always be an object since we constructed it above
                                return Ok(());
                            };
                            for key in ALLOWED_PROJECT_KEYS {
                                if let Some(value) = project_obj.get(*key) {
                                    settings_obj.insert((*key).to_string(), value.clone());
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Silently skip unparseable project settings to avoid
                        // writing to stderr which corrupts the TUI display.
                    }
                },
                Err(_) => {
                    // Silently skip unreadable project settings.
                }
            }
        }

        fs::write(
            claude_dir.join("settings.json"),
            serde_json::to_string_pretty(&settings)?,
        )?;

        Ok(())
    }

    /// Write `.gemini/settings.json` in the task directory with hooks that
    /// support task management (prompt activity detection, agent stop detection,
    /// PR link discovery). Mirrors Claude Code's hook structure using Gemini CLI
    /// hook equivalents: BeforeAgent → UserPromptSubmit, AfterAgent → Stop,
    /// AfterTool → PostToolUse.
    pub fn write_gemini_hooks(&self, task: &Task) -> AppResult<()> {
        let task_dir = self.task_dir(&task.project_id, &task.id);

        let gemini_dir = task_dir.join(".gemini");
        fs::create_dir_all(&gemini_dir)?;

        let pr_links_path = task_dir.join(".pr_links");
        let pr_links_path_str = pr_links_path.to_string_lossy();
        let prompt_submitted_path = task_dir.join(".prompt_submitted");
        let prompt_submitted_path_str = prompt_submitted_path.to_string_lossy();
        let agent_stopped_path = task_dir.join(".agent_stopped");
        let agent_stopped_path_str = agent_stopped_path.to_string_lossy();

        let mut settings = serde_json::json!({
            "hooks": {
                "BeforeAgent": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!(
                                    "touch {} && rm -f {}",
                                    shell_escape(&prompt_submitted_path_str),
                                    shell_escape(&agent_stopped_path_str)
                                )
                            }
                        ]
                    }
                ],
                "AfterAgent": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!(
                                    "touch {}",
                                    shell_escape(&agent_stopped_path_str)
                                )
                            }
                        ]
                    }
                ],
                "AfterTool": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!(
                                    "grep -oE 'https://github\\.com/[^\"/]+/[^\"/]+/pull/[0-9]+' | grep -vE '/(owner|org|example|user|your-org)/(repo|repository|my-repo|your-repo|example)/' >> {} || true",
                                    shell_escape(&pr_links_path_str)
                                )
                            }
                        ]
                    }
                ]
            }
        });

        // Merge allowed settings from project-level .gemini/settings.json
        // (mirrors write_claude_hooks behaviour for symmetry).
        const ALLOWED_PROJECT_KEYS: &[&str] = &["extensions", "mcpServers"];

        let project_settings_path = self
            .project_dir(&task.project_id)
            .join(".gemini")
            .join("settings.json");
        if project_settings_path.exists() {
            match fs::read_to_string(&project_settings_path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(project_settings) => {
                        if let Some(project_obj) = project_settings.as_object() {
                            let Some(settings_obj) = settings.as_object_mut() else {
                                return Ok(());
                            };
                            for key in ALLOWED_PROJECT_KEYS {
                                if let Some(value) = project_obj.get(*key) {
                                    settings_obj.insert((*key).to_string(), value.clone());
                                }
                            }
                        }
                    }
                    Err(_) => {}
                },
                Err(_) => {}
            }
        }

        fs::write(
            gemini_dir.join("settings.json"),
            serde_json::to_string_pretty(&settings)?,
        )?;

        Ok(())
    }

    /// Copy `.claude/settings.local.json` from the project directory to the
    /// task directory so that project-local settings (e.g. plugin
    /// configurations not committed to version control) are inherited.
    fn copy_claude_settings_local(&self, task: &Task) -> AppResult<()> {
        let project_local = self
            .project_dir(&task.project_id)
            .join(".claude")
            .join("settings.local.json");
        if !project_local.exists() {
            return Ok(());
        }

        let task_claude_dir = self
            .task_dir(&task.project_id, &task.id)
            .join(".claude");
        fs::create_dir_all(&task_claude_dir)?;

        match fs::read_to_string(&project_local) {
            Ok(content) => {
                // Validate it is proper JSON before copying
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(_) => {
                        fs::write(task_claude_dir.join("settings.local.json"), content)?;
                    }
                    Err(_) => {
                        // Silently skip unparseable settings.local.json to avoid
                        // writing to stderr which corrupts the TUI display.
                    }
                }
            }
            Err(_) => {
                // Silently skip unreadable settings.local.json.
            }
        }

        Ok(())
    }

    /// Generate the shared body of the task-management SKILL.md.
    fn skill_body(task: &Task) -> String {
        format!(
            r#"# Task Management Skill

You are working inside a **my-agents** managed session.

- **Task ID**: `{task_id}`
- **Project ID**: `{project_id}`

## CLI: `ma-task`

Use the `ma-task` command to manage tasks. Output is JSON.

### Get current task info

```bash
ma-task current
```

### Update task status

```bash
ma-task status {task_id} <status>
```

Valid statuses:

- **Todo** — Not started yet.
- **InProgress** — Currently being worked on.
- **ActionRequired** — Waiting for user action or review. Set this when you finish your work and need user input.
- **Completed** — All PRs merged and work fully done. Usually set automatically by the system.
- **Blocked** — Stopped due to an external dependency. Do not set this automatically; only when you truly cannot proceed.

### Add a link (PR, issue, etc.)

```bash
ma-task link {task_id} <url>
ma-task link {task_id} <url> --name "PR #123"
```

### Register a preview URL

```bash
ma-task preview-url {task_id} <url> --name <service-name>
```

Example:
```bash
ma-task preview-url {task_id} http://localhost:3000 --name web
```

### Get a specific task

```bash
ma-task get <task-id>
```

### List all tasks in this project

```bash
ma-task list --project {project_id}
```

### Create a new task

```bash
ma-task create --project {project_id} --name "task name" [--priority P1-P5] [--agent Claude|Codex|Gemini|None]
```

### Update task fields

```bash
ma-task update {task_id} --name "new name" --priority P2 --notes "some notes" --agent Claude
```

### Run an existing task

```bash
ma-task run <task-id>
```

Sets up worktree, tmux session, and launches the agent for an existing task (same as `create --run` but for tasks that already exist).

### Delete a task

```bash
ma-task delete <task-id>
```

Deletes a task and cleans up its worktrees and tmux session.

### List all projects

```bash
ma-task projects
```

## Guidelines

- After creating a PR, always add the link with `ma-task link`.
"#,
            task_id = task.id,
            project_id = task.project_id,
        )
    }

    /// Write `.claude/skills/task-management/SKILL.md` in the task directory.
    fn write_claude_skill(&self, task: &Task) -> AppResult<()> {
        let task_dir = self.task_dir(&task.project_id, &task.id);
        let skill_dir = task_dir.join(".claude").join("skills").join("task-management");
        fs::create_dir_all(&skill_dir)?;

        let skill_md = format!(
            "---\n\
             name: task-management\n\
             description: \"Use when you need to check your task details, update task status, \
             add links (PR/issue URLs), or create/list tasks in the project.\"\n\
             allowed-tools: Bash\n\
             ---\n\n{}",
            Self::skill_body(task),
        );

        fs::write(skill_dir.join("SKILL.md"), skill_md)?;
        Ok(())
    }

    /// Write `.agents/skills/task-management/SKILL.md` in the task directory for Codex.
    fn write_codex_skill(&self, task: &Task) -> AppResult<()> {
        let task_dir = self.task_dir(&task.project_id, &task.id);
        let skill_dir = task_dir
            .join(".agents")
            .join("skills")
            .join("task-management");
        fs::create_dir_all(&skill_dir)?;

        let skill_md = format!(
            "---\n\
             name: task-management\n\
             description: \"Use when you need to check your task details, update task status, \
             add links (PR/issue URLs), or create/list tasks in the project.\"\n\
             ---\n\n{}",
            Self::skill_body(task),
        );

        fs::write(skill_dir.join("SKILL.md"), skill_md)?;
        Ok(())
    }

    /// Write `.gemini/skills/task-management/SKILL.md` in the task directory for Gemini CLI.
    fn write_gemini_skill(&self, task: &Task) -> AppResult<()> {
        let task_dir = self.task_dir(&task.project_id, &task.id);
        let skill_dir = task_dir
            .join(".gemini")
            .join("skills")
            .join("task-management");
        fs::create_dir_all(&skill_dir)?;

        let skill_md = format!(
            "---\n\
             name: task-management\n\
             description: \"Use when you need to check your task details, update task status, \
             add links (PR/issue URLs), or create/list tasks in the project.\"\n\
             ---\n\n{}",
            Self::skill_body(task),
        );

        fs::write(skill_dir.join("SKILL.md"), skill_md)?;
        Ok(())
    }

    /// Generate the body for the dev-environment skill.
    fn dev_env_skill_body(task: &Task, prompt: &str) -> String {
        format!(
            r#"# Dev Environment Skill

You are working inside a **my-agents** managed session.

- **Task ID**: `{task_id}`
- **Project ID**: `{project_id}`

## How to start the dev environment

{prompt}

## Registering Preview URLs

After starting the development server, register the preview URL(s) so the user can open them from the TUI.

Use the `ma-task` command to register each service's preview URL:

```bash
ma-task preview-url {task_id} <url> --name <service-name>
```

For example:
```bash
ma-task preview-url {task_id} http://localhost:3000 --name web
ma-task preview-url {task_id} http://localhost:8080 --name api
```

## Guidelines

- Start the dev environment as instructed above.
- Register all preview URLs after the services are running.
- If a service restarts on a different port, update the preview URL.
"#,
            task_id = task.id,
            project_id = task.project_id,
            prompt = prompt,
        )
    }

    /// Write `.claude/skills/dev-environment/SKILL.md` in the task directory.
    fn write_dev_env_skill_claude(dir: &std::path::Path, task: &Task, prompt: &str) -> AppResult<()> {
        let skill_dir = dir.join(".claude").join("skills").join("dev-environment");
        fs::create_dir_all(&skill_dir)?;

        let skill_md = format!(
            "---\n\
             name: dev-environment\n\
             description: \"Use to start the development environment and register preview URLs for this project.\"\n\
             allowed-tools: Bash\n\
             ---\n\n{}",
            Self::dev_env_skill_body(task, prompt),
        );

        fs::write(skill_dir.join("SKILL.md"), skill_md)?;
        Ok(())
    }

    /// Write `.agents/skills/dev-environment/SKILL.md` in the task directory for Codex.
    fn write_dev_env_skill_codex(dir: &std::path::Path, task: &Task, prompt: &str) -> AppResult<()> {
        let skill_dir = dir
            .join(".agents")
            .join("skills")
            .join("dev-environment");
        fs::create_dir_all(&skill_dir)?;

        let skill_md = format!(
            "---\n\
             name: dev-environment\n\
             description: \"Use to start the development environment and register preview URLs for this project.\"\n\
             ---\n\n{}",
            Self::dev_env_skill_body(task, prompt),
        );

        fs::write(skill_dir.join("SKILL.md"), skill_md)?;
        Ok(())
    }

    /// Write `.gemini/skills/dev-environment/SKILL.md` in the task directory for Gemini CLI.
    fn write_dev_env_skill_gemini(dir: &std::path::Path, task: &Task, prompt: &str) -> AppResult<()> {
        let skill_dir = dir
            .join(".gemini")
            .join("skills")
            .join("dev-environment");
        fs::create_dir_all(&skill_dir)?;

        let skill_md = format!(
            "---\n\
             name: dev-environment\n\
             description: \"Use to start the development environment and register preview URLs for this project.\"\n\
             ---\n\n{}",
            Self::dev_env_skill_body(task, prompt),
        );

        fs::write(skill_dir.join("SKILL.md"), skill_md)?;
        Ok(())
    }

    // PM (Project Manager) methods

    pub fn pm_dir(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("pm")
    }

    pub fn write_pm_config_files(&self, project: &Project) -> AppResult<()> {
        let project_dir = self.project_dir(&project.id);
        fs::create_dir_all(&project_dir)?;

        let agent_cli = project.pm_agent_cli.unwrap_or(crate::domain::task::AgentCli::Claude);
        let custom_instructions = project.pm_custom_instructions.as_deref().unwrap_or("");

        let pm_config_body = Self::pm_config_body(project, custom_instructions);
        let pm_skill = Self::pm_skill_body(project);

        match agent_cli {
            crate::domain::task::AgentCli::Claude => {
                // Write CLAUDE.md to project dir (PM runs here directly)
                fs::write(project_dir.join("CLAUDE.md"), &pm_config_body)?;

                // Write PM skill to project dir
                let skill_dir = project_dir.join(".claude").join("skills").join("pm-manager");
                fs::create_dir_all(&skill_dir)?;
                let skill_md = format!(
                    "---\n\
                     name: pm-manager\n\
                     description: \"Use to check project task progress, analyze agent sessions, and provide status reports with recommendations.\"\n\
                     allowed-tools: Bash\n\
                     ---\n\n{}",
                    pm_skill,
                );
                fs::write(skill_dir.join("SKILL.md"), skill_md)?;

                // No separate hooks needed — PM inherits project-level settings.json

                // Trust project dir
                Self::ensure_claude_trust(&project_dir)?;
            }
            crate::domain::task::AgentCli::Codex => {
                // Write AGENTS.md to project dir
                fs::write(project_dir.join("AGENTS.md"), &pm_config_body)?;

                // Write PM skill to project dir
                let skill_dir = project_dir.join(".agents").join("skills").join("pm-manager");
                fs::create_dir_all(&skill_dir)?;
                let skill_md = format!(
                    "---\n\
                     name: pm-manager\n\
                     description: Use to check project task progress, analyze agent sessions, and provide status reports with recommendations.\n\
                     ---\n\n{}",
                    pm_skill,
                );
                fs::write(skill_dir.join("SKILL.md"), skill_md)?;
            }
            crate::domain::task::AgentCli::Gemini => {
                // Write GEMINI.md to project dir
                fs::write(project_dir.join("GEMINI.md"), &pm_config_body)?;

                // Write PM skill to project dir
                let skill_dir = project_dir.join(".gemini").join("skills").join("pm-manager");
                fs::create_dir_all(&skill_dir)?;
                let skill_md = format!(
                    "---\n\
                     name: pm-manager\n\
                     description: Use to check project task progress, analyze agent sessions, and provide status reports with recommendations.\n\
                     ---\n\n{}",
                    pm_skill,
                );
                fs::write(skill_dir.join("SKILL.md"), skill_md)?;
            }
            crate::domain::task::AgentCli::None => {}
        }

        Ok(())
    }

    fn pm_config_body(project: &Project, custom_instructions: &str) -> String {
        let mut lines = Vec::new();
        lines.push("# Project Manager Agent".to_string());
        lines.push(String::new());
        lines.push(format!(
            "You are the Project Manager (PM) for the **{}** project.",
            project.name
        ));
        lines.push("You are triggered periodically to review task progress and provide recommendations.".to_string());
        lines.push(String::new());
        lines.push("## Your Responsibilities".to_string());
        lines.push(String::new());
        lines.push("1. Review the current status of all tasks in this project".to_string());
        lines.push("2. Check agent session outputs for progress updates".to_string());
        lines.push("3. Identify blocked or stalled tasks".to_string());
        lines.push("4. Provide a concise status report with actionable recommendations".to_string());
        lines.push(String::new());

        let agent_cli = project.pm_agent_cli.unwrap_or(crate::domain::task::AgentCli::Claude);
        match agent_cli {
            crate::domain::task::AgentCli::Claude => {
                lines.push("## How to Start".to_string());
                lines.push(String::new());
                lines.push("Use the `/pm-manager` skill to perform your review.".to_string());
            }
            crate::domain::task::AgentCli::Codex => {
                lines.push("## How to Start".to_string());
                lines.push(String::new());
                lines.push("Use the `$pm-manager` skill to perform your review.".to_string());
            }
            crate::domain::task::AgentCli::Gemini => {
                lines.push("## How to Start".to_string());
                lines.push(String::new());
                lines.push("Use the pm-manager skill to perform your review.".to_string());
            }
            _ => {}
        }

        if !custom_instructions.is_empty() {
            lines.push(String::new());
            lines.push("## Custom Instructions".to_string());
            lines.push(String::new());
            lines.push(custom_instructions.to_string());
        }

        lines.join("\n") + "\n"
    }

    fn pm_skill_body(project: &Project) -> String {
        format!(
            r#"# PM Manager Skill

You are the Project Manager for **{project_id}**.

## Step 1: Get Task List

```bash
ma-task list --project {project_id}
```

Review the JSON output to understand all tasks and their statuses.

## Step 2: Check Agent Sessions

For each task that has a tmux session, capture its recent output:

```bash
ma-task list --project {project_id} | jq -r '.[] | select(.tmux_session != null) | .tmux_session' | while read session; do
  echo "=== Session: $session ==="
  tmux capture-pane -t "$session" -p 2>/dev/null || echo "(session not active)"
  echo ""
done
```

## Step 3: Analyze and Report

Based on the task list and session outputs, provide:

1. **Status Summary**: Brief overview of each task's progress
2. **Stalled Tasks**: Identify tasks that appear stuck or have no recent activity
3. **Action Items**: Specific recommendations (e.g., "Task X needs review", "Task Y is blocked on Z")
4. **Priority Adjustments**: Suggest priority changes if needed

## Step 4: Take Action (if appropriate)

You can update task statuses or create new tasks:

```bash
ma-task status <task-id> <status>
ma-task create --project {project_id} --name "task name" --priority P3
```

## Guidelines

- Be concise and actionable
- Skip tasks that have no changes since last check
- Focus on tasks that need attention (InProgress, ActionRequired, Blocked)
- Do not modify tasks that are Completed unless there's a clear issue
"#,
            project_id = project.id,
        )
    }


    pub fn ensure_quickstart(&self) -> AppResult<()> {
        let projects = self.list_projects()?;
        if !projects.is_empty() {
            return Ok(());
        }

        let now = chrono::Utc::now();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

        // Create quickstart project
        let project = Project {
            id: "quickstart".to_string(),
            name: "quickstart".to_string(),
            repos: vec![crate::domain::project::RepoRef {
                name: "home".to_string(),
                path: home,
            }],
            description: None,
            worktree_copy_files: Vec::new(),
            dev_environment_prompt: None,
            pm_enabled: false,
            pm_agent_cli: None,
            pm_custom_instructions: None,
            pm_cron_expression: None,
            pm_tmux_session: None,
            created_at: now,
            updated_at: now,
        };
        self.save_project(&project)?;

        // Create how-to task
        let task = Task {
            id: "howto1".to_string(),
            project_id: "quickstart".to_string(),
            name: "how-to".to_string(),
            priority: crate::domain::task::Priority::P3,
            status: crate::domain::task::Status::Todo,
            agent_cli: crate::domain::task::AgentCli::None,
            worktrees: vec![],
            links: vec![],
            preview_urls: vec![],
            notes: Some(
                "Welcome to my-agents!\n\
                 \n\
                 Quick Start:\n\
                 \n\
                 1. Press 'p' to create a new project\n\
                    - Give it a name and add git repository paths\n\
                 \n\
                 2. Press 'n' to add a task to the project\n\
                    - Choose an agent CLI (Claude, Codex, Gemini, or None)\n\
                    - A tmux session is created automatically\n\
                 \n\
                 3. Press Enter to attach to a task's session\n\
                    - Work inside the tmux session\n\
                    - Press Ctrl+Q to detach back to this screen\n\
                 \n\
                 Key Bindings:\n\
                 \n\
                   p - Create project    n - Add task\n\
                   m - Edit item         d - Delete item\n\
                   S - Set status        L - Add link\n\
                   f - Filter tasks      s - Sort tasks\n\
                   o - Open link         q - Quit\n\
                 \n\
                 Tip: You can delete this quickstart project\n\
                 once you've created your own!"
                    .to_string(),
            ),
            initial_instructions: None,
            tmux_session: None,
            created_at: now,
            updated_at: now,
            reopened_at: None,
        };
        self.save_task(&task)?;

        Ok(())
    }
}

/// Simple shell escaping: wrap in single quotes, escaping any existing single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
