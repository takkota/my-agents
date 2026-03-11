use crate::config::Config;
use crate::domain::project::Project;
use crate::domain::task::Task;
use crate::error::AppResult;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

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
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
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
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    pub fn write_agent_config_files(&self, task: &Task) -> AppResult<()> {
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
            claude_lines.push(String::new());
            claude_lines.push("## Pull Request".to_string());
            claude_lines.push(
                "If any code changes were made during this task, you MUST create a Pull Request \
                 before marking the task as completed."
                    .to_string(),
            );
        }
        if !claude_lines.is_empty() {
            fs::write(dir.join("CLAUDE.md"), claude_lines.join("\n") + "\n")?;
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
            agents_lines.push(String::new());
            agents_lines.push("## Pull Request".to_string());
            agents_lines.push(
                "If any code changes were made during this task, you MUST create a Pull Request \
                 before marking the task as completed."
                    .to_string(),
            );
        }
        if !agents_lines.is_empty() {
            fs::write(dir.join("AGENTS.md"), agents_lines.join("\n") + "\n")?;
        }

        // Write Claude Code hooks config and skill for Claude agent tasks
        if task.agent_cli == crate::domain::task::AgentCli::Claude {
            self.write_claude_hooks(task)?;
            self.write_claude_skill(task)?;
        }

        // Write Codex skill and configure notify for Codex agent tasks
        if task.agent_cli == crate::domain::task::AgentCli::Codex {
            self.write_codex_skill(task)?;
            self.write_codex_notify(task)?;
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

        Ok(())
    }

    /// Copy skill subdirectories from `src` into `dst`.
    /// Each child directory in `src` is symlinked into `dst` unless a
    /// directory with the same name already exists (e.g. task-management).
    /// Errors on individual entries are logged and skipped rather than
    /// aborting the entire operation.
    fn copy_skills_dir(src: &std::path::Path, dst: &std::path::Path) -> AppResult<()> {
        for entry in fs::read_dir(src)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name();
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
            if let Err(e) = std::os::unix::fs::symlink(&canonical, &dest) {
                eprintln!(
                    "Warning: failed to symlink skill {:?} -> {:?}: {}",
                    canonical, dest, e
                );
            }
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
    /// PR link discovery).
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

        let settings = serde_json::json!({
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
                                    "grep -oE 'https://github\\.com/[^\"/]+/[^\"/]+/pull/[0-9]+' | grep -v '/owner/repo/' >> {} || true",
                                    shell_escape(&pr_links_path_str)
                                )
                            }
                        ]
                    }
                ]
            }
        });

        fs::write(
            claude_dir.join("settings.json"),
            serde_json::to_string_pretty(&settings)?,
        )?;

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
ma-task create --project {project_id} --name "task name" [--priority P1-P5] [--agent Claude|Codex|None]
```

### Update task fields

```bash
ma-task update {task_id} --name "new name" --priority P2 --notes "some notes"
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
            worktree_copy_files: Vec::new(),
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
            notes: Some(
                "Welcome to my-agents!\n\
                 \n\
                 Quick Start:\n\
                 \n\
                 1. Press 'p' to create a new project\n\
                    - Give it a name and add git repository paths\n\
                 \n\
                 2. Press 'n' to add a task to the project\n\
                    - Choose an agent CLI (Claude, Codex, or None)\n\
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
