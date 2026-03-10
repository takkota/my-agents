use crate::config::Config;
use crate::domain::project::Project;
use crate::domain::task::Task;
use crate::error::AppResult;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FsStore {
    projects_dir: PathBuf,
}

impl FsStore {
    pub fn new(config: &Config) -> AppResult<Self> {
        let projects_dir = config.projects_dir();
        fs::create_dir_all(&projects_dir)?;
        Ok(Self { projects_dir })
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

        // Write CLAUDE.md with references
        let claude_lines: Vec<String> = task
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
        if !claude_lines.is_empty() {
            fs::write(dir.join("CLAUDE.md"), claude_lines.join("\n") + "\n")?;
        }

        // Write AGENTS.md with references
        let agents_lines: Vec<String> = task
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
        if !agents_lines.is_empty() {
            fs::write(dir.join("AGENTS.md"), agents_lines.join("\n") + "\n")?;
        }

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
            notes: None,
            tmux_session: None,
            created_at: now,
            updated_at: now,
        };
        self.save_task(&task)?;

        Ok(())
    }
}
