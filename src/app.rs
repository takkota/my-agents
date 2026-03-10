use crate::action::Action;
use crate::components::modals::confirm_delete::{ConfirmDeleteModal, DeleteTarget};
use crate::components::modals::create_project::CreateProjectModal;
use crate::components::modals::create_task::CreateTaskModal;
use crate::components::modals::edit_item::{EditItemModal, EditProjectModal, EditTaskModal};
use crate::components::modals::filter::FilterModal;
use crate::components::modals::select_link::SelectLinkModal;
use crate::components::modals::set_link::SetLinkModal;
use crate::components::modals::set_status::SetStatusModal;
use crate::components::modals::sort::SortModal;
use crate::components::modals::{centered_rect, Modal};
use crate::components::preview_panel::PreviewPanel;
use crate::components::status_bar::StatusBar;
use crate::components::task_tree::{TaskTree, TreeItem};
use crate::config::Config;
use crate::domain::project::{Project, RepoRef};
use crate::domain::task::{AgentCli, Task};
use crate::error::AppResult;
use crate::services::git_finder;
use crate::services::tmux::TmuxService;
use crate::services::worktree::WorktreeService;
use crate::storage::FsStore;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Result of App::update() that tells the main loop what to do next
pub enum UpdateResult {
    Continue,
    AttachSession(String),
}

pub struct App {
    pub running: bool,
    pub config: Config,
    pub store: FsStore,
    pub tmux: TmuxService,
    pub worktree_svc: WorktreeService,

    // Data
    pub projects: Vec<Project>,
    pub tasks_by_project: HashMap<String, Vec<Task>>,

    // UI components
    pub task_tree: TaskTree,
    pub preview_panel: PreviewPanel,

    // Modal state
    pub active_modal: Option<ModalKind>,

    // Cached
    pub available_repos: Vec<std::path::PathBuf>,
    pub active_sessions: HashSet<String>,

    // Error display
    pub error_message: Option<String>,
}

pub enum ModalKind {
    CreateProject(CreateProjectModal),
    CreateTask(CreateTaskModal),
    EditItem(EditItemModal),
    SetStatus(SetStatusModal),
    SetLink(SetLinkModal),
    SelectLink(SelectLinkModal),
    Filter(FilterModal),
    Sort(SortModal),
    ConfirmDelete(ConfirmDeleteModal),
}

impl App {
    pub fn new(config: Config) -> AppResult<Self> {
        let store = FsStore::new(&config)?;
        store.ensure_quickstart()?;
        let tmux = TmuxService::new();
        let worktree_svc = WorktreeService::new();

        let mut app = Self {
            running: true,
            config,
            store,
            tmux,
            worktree_svc,
            projects: Vec::new(),
            tasks_by_project: HashMap::new(),
            task_tree: TaskTree::new(),
            preview_panel: PreviewPanel::new(),
            active_modal: None,
            available_repos: Vec::new(),
            active_sessions: HashSet::new(),
            error_message: None,
        };

        app.reload_data()?;

        // Expand first project by default
        if let Some(p) = app.projects.first() {
            app.task_tree.expanded.insert(p.id.clone());
        }
        app.rebuild_tree();

        Ok(app)
    }

    pub fn reload_data(&mut self) -> AppResult<()> {
        self.projects = self.store.list_projects()?;
        self.tasks_by_project.clear();
        for project in &self.projects {
            let tasks = self.store.list_tasks(&project.id)?;
            self.tasks_by_project.insert(project.id.clone(), tasks);
        }

        // Refresh active tmux sessions
        self.active_sessions.clear();
        if let Ok(sessions) = self.tmux.list_sessions() {
            for s in sessions {
                self.active_sessions.insert(s);
            }
        }

        Ok(())
    }

    fn rebuild_tree(&mut self) {
        self.task_tree
            .rebuild(&self.projects, &self.tasks_by_project, &self.active_sessions);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // Modal takes priority
        if let Some(modal) = &mut self.active_modal {
            return match modal {
                ModalKind::CreateProject(m) => m.handle_key(key),
                ModalKind::CreateTask(m) => m.handle_key(key),
                ModalKind::EditItem(m) => m.handle_key(key),
                ModalKind::SetStatus(m) => m.handle_key(key),
                ModalKind::SetLink(m) => m.handle_key(key),
                ModalKind::SelectLink(m) => m.handle_key(key),
                ModalKind::Filter(m) => m.handle_key(key),
                ModalKind::Sort(m) => m.handle_key(key),
                ModalKind::ConfirmDelete(m) => m.handle_key(key),
            };
        }

        // Global keys
        match key.code {
            KeyCode::Char('q') => return Ok(Some(Action::Quit)),
            KeyCode::Char('p') => return Ok(Some(Action::OpenCreateProject)),
            KeyCode::Char('n') => return Ok(Some(Action::OpenCreateTask)),
            KeyCode::Char('m') => return Ok(Some(Action::OpenEditItem)),
            KeyCode::Char('d') => return Ok(Some(Action::OpenConfirmDelete)),
            KeyCode::Char('f') => return Ok(Some(Action::OpenFilter)),
            KeyCode::Char('s') => return Ok(Some(Action::OpenSort)),
            KeyCode::Char('S') | KeyCode::Char('$') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('S') {
                    return Ok(Some(Action::OpenSetStatus));
                }
            }
            KeyCode::Char('L') => {
                return Ok(Some(Action::OpenSetLink));
            }
            KeyCode::Char('o') => {
                // Open task link(s) in external browser
                if let Some(TreeItem::Task { id, project_id, .. }) =
                    self.task_tree.selected_item()
                {
                    if let Some(tasks) = self.tasks_by_project.get(project_id.as_str()) {
                        if let Some(task) = tasks.iter().find(|t| t.id == *id) {
                            match task.links.len() {
                                0 => {}
                                1 => {
                                    return Ok(Some(Action::OpenLinkInBrowser {
                                        url: task.links[0].url.clone(),
                                    }));
                                }
                                _ => {
                                    self.active_modal = Some(ModalKind::SelectLink(
                                        SelectLinkModal::new(task.links.clone()),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => return Ok(Some(Action::MoveUp)),
            KeyCode::Down | KeyCode::Char('j') => return Ok(Some(Action::MoveDown)),
            KeyCode::Enter => return Ok(Some(Action::AttachSession)),
            _ => {}
        }

        Ok(None)
    }

    pub fn update(&mut self, action: Action) -> AppResult<UpdateResult> {
        self.error_message = None;
        match action {
            Action::Quit => {
                self.running = false;
            }
            Action::MoveUp => {
                self.task_tree.move_up();
            }
            Action::MoveDown => {
                self.task_tree.move_down();
            }
            Action::ToggleExpand => {
                self.task_tree.toggle_expand();
            }
            Action::AttachSession => {
                if let Some(name) = self.resolve_attach_session() {
                    return Ok(UpdateResult::AttachSession(name));
                }
            }

            // Modal openers
            Action::OpenCreateProject => {
                if self.available_repos.is_empty() {
                    self.available_repos = git_finder::find_git_repos().unwrap_or_default();
                }
                self.active_modal = Some(ModalKind::CreateProject(CreateProjectModal::new(
                    self.available_repos.clone(),
                )));
            }
            Action::OpenCreateTask => {
                if self.projects.is_empty() {
                    // No projects yet - prompt to create one first
                    if self.available_repos.is_empty() {
                        self.available_repos = git_finder::find_git_repos().unwrap_or_default();
                    }
                    self.active_modal = Some(ModalKind::CreateProject(CreateProjectModal::new(
                        self.available_repos.clone(),
                    )));
                    return Ok(UpdateResult::Continue);
                }

                let project_id = self
                    .task_tree
                    .selected_item()
                    .map(|item| item.project_id().to_string())
                    .unwrap_or_else(|| self.projects[0].id.clone());

                self.active_modal = Some(ModalKind::CreateTask(CreateTaskModal::new(
                    project_id,
                    self.config.default_agent_cli,
                )));
            }
            Action::OpenEditItem => {
                if let Some(item) = self.task_tree.selected_item().cloned() {
                    match item {
                        TreeItem::Project { id, name, .. } => {
                            if self.available_repos.is_empty() {
                                self.available_repos =
                                    git_finder::find_git_repos().unwrap_or_default();
                            }
                            let project = self.projects.iter().find(|p| p.id == id);
                            let selected_repos: Vec<std::path::PathBuf> = project
                                .map(|p| p.repos.iter().map(|r| r.path.clone()).collect())
                                .unwrap_or_default();

                            self.active_modal = Some(ModalKind::EditItem(EditItemModal::Project(
                                EditProjectModal::new(
                                    id,
                                    name,
                                    self.available_repos.clone(),
                                    selected_repos,
                                ),
                            )));
                        }
                        TreeItem::Task {
                            id,
                            project_id,
                            name,
                            notes,
                            ..
                        } => {
                            self.active_modal = Some(ModalKind::EditItem(EditItemModal::Task(
                                EditTaskModal::new(id, project_id, name, notes),
                            )));
                        }
                    }
                }
            }
            Action::OpenSetStatus => {
                if let Some(TreeItem::Task {
                    id,
                    project_id,
                    status,
                    ..
                }) = self.task_tree.selected_item().cloned()
                {
                    self.active_modal = Some(ModalKind::SetStatus(SetStatusModal::new(
                        id, project_id, status,
                    )));
                }
            }
            Action::OpenSetLink => {
                if let Some(TreeItem::Task { id, project_id, .. }) =
                    self.task_tree.selected_item().cloned()
                {
                    self.active_modal =
                        Some(ModalKind::SetLink(SetLinkModal::new(id, project_id)));
                }
            }
            Action::OpenFilter => {
                let current = self.task_tree.status_filter.as_deref();
                self.active_modal = Some(ModalKind::Filter(FilterModal::new(current)));
            }
            Action::OpenSort => {
                self.active_modal =
                    Some(ModalKind::Sort(SortModal::new(self.task_tree.sort_mode)));
            }
            Action::OpenConfirmDelete => {
                if let Some(item) = self.task_tree.selected_item().cloned() {
                    let target = match item {
                        TreeItem::Project { id, name, .. } => {
                            DeleteTarget::Project { id, name }
                        }
                        TreeItem::Task {
                            id,
                            project_id,
                            name,
                            ..
                        } => DeleteTarget::Task {
                            project_id,
                            task_id: id,
                            name,
                        },
                    };
                    self.active_modal =
                        Some(ModalKind::ConfirmDelete(ConfirmDeleteModal::new(target)));
                }
            }

            // Close modal without applying (Esc = cancel)
            Action::CloseModal => {
                self.active_modal = None;
                self.rebuild_tree();
            }

            // Close modal and apply changes (Enter = confirm for filter/sort)
            Action::ApplyAndCloseModal => {
                if let Some(ModalKind::Filter(ref modal)) = self.active_modal {
                    self.task_tree.status_filter = modal.selected_statuses();
                }
                if let Some(ModalKind::Sort(ref modal)) = self.active_modal {
                    self.task_tree.sort_mode = modal.selected_mode();
                }
                self.active_modal = None;
                self.rebuild_tree();
            }

            // CRUD actions
            Action::CreateProject { name, repos } => {
                // Check for duplicate project ID
                if self.projects.iter().any(|p| p.id == name) {
                    self.error_message = Some(format!("Project '{}' already exists", name));
                    self.active_modal = None;
                    return Ok(UpdateResult::Continue);
                }
                let now = Utc::now();
                let project = Project {
                    id: name.clone(),
                    name: name.clone(),
                    repos: repos
                        .into_iter()
                        .map(|(n, p)| RepoRef { name: n, path: p })
                        .collect(),
                    created_at: now,
                    updated_at: now,
                };
                self.store.save_project(&project)?;
                self.active_modal = None;
                self.reload_data()?;
                self.task_tree.expanded.insert(project.id.clone());
                self.rebuild_tree();
            }
            Action::UpdateProject {
                project_id,
                name,
                repos,
            } => {
                if let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() {
                    let mut updated = project;
                    updated.name = name;
                    updated.repos = repos
                        .into_iter()
                        .map(|(n, p)| RepoRef { name: n, path: p })
                        .collect();
                    updated.updated_at = Utc::now();
                    self.store.save_project(&updated)?;
                }
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::DeleteProject { project_id } => {
                // Kill tmux sessions and remove worktrees for all tasks
                if let Some(tasks) = self.tasks_by_project.get(&project_id) {
                    for task in tasks {
                        self.cleanup_task(task)?;
                    }
                }
                self.store.delete_project(&project_id)?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }

            Action::CreateTask {
                project_id,
                name,
                priority,
                agent_cli,
                notes,
            } => {
                self.handle_create_task(project_id, name, priority, agent_cli, notes)?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::UpdateTask {
                task_id,
                project_id,
                name,
                notes,
            } => {
                self.update_task(&project_id, &task_id, |task| {
                    task.name = name;
                    task.notes = notes;
                    task.updated_at = Utc::now();
                })?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::UpdateTaskStatus {
                task_id,
                project_id,
                status,
            } => {
                self.update_task(&project_id, &task_id, |task| {
                    task.status = status;
                    task.updated_at = Utc::now();
                })?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::UpdateTaskLink {
                task_id,
                project_id,
                link,
            } => {
                self.update_task(&project_id, &task_id, |task| {
                    task.links.push(link);
                    task.updated_at = Utc::now();
                })?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::DeleteTask {
                project_id,
                task_id,
            } => {
                if let Some(tasks) = self.tasks_by_project.get(&project_id) {
                    if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
                        self.cleanup_task(task)?;
                    }
                }
                self.store.delete_task_dir(&project_id, &task_id)?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }

            Action::OpenLinkInBrowser { url } => {
                self.active_modal = None;
                // Open URL in external browser using xdg-open (Linux) or open (macOS)
                let cmd = if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                };
                if let Err(e) = std::process::Command::new(cmd).arg(&url).spawn() {
                    self.error_message = Some(format!("Failed to open link: {}", e));
                }
            }

            Action::AgentStatusChanged {
                task_id,
                project_id,
                status,
            } => {
                self.update_task(&project_id, &task_id, |task| {
                    task.status = status;
                    task.updated_at = Utc::now();
                })?;
                self.reload_data()?;
                self.rebuild_tree();
            }

            Action::Tick => {
                // Refresh active sessions periodically
                self.active_sessions.clear();
                if let Ok(sessions) = self.tmux.list_sessions() {
                    for s in sessions {
                        self.active_sessions.insert(s);
                    }
                }

                // Update preview with task info
                let selected_task = self.task_tree.selected_item().and_then(|item| {
                    if let TreeItem::Task { id, project_id, .. } = item {
                        self.tasks_by_project
                            .get(project_id.as_str())
                            .and_then(|tasks| tasks.iter().find(|t| t.id == *id))
                    } else {
                        None
                    }
                });

                let session_name_owned = selected_task
                    .and_then(|t| t.tmux_session.as_deref())
                    .map(|s| s.to_string());

                // Update task info (links + notes)
                if let Some(task) = selected_task {
                    self.preview_panel
                        .update_task_info(task.links.clone(), task.notes.clone());
                } else {
                    self.preview_panel.update_task_info(Vec::new(), None);
                }

                self.preview_panel
                    .update_preview(session_name_owned.as_deref(), &self.tmux);
            }

            Action::Noop => {}
        }

        Ok(UpdateResult::Continue)
    }

    fn handle_create_task(
        &mut self,
        project_id: String,
        name: String,
        priority: crate::domain::task::Priority,
        agent_cli: AgentCli,
        notes: Option<String>,
    ) -> AppResult<()> {
        // Generate unique 8-char task ID, checking for collisions
        let task_id = loop {
            let candidate = Uuid::new_v4().to_string()[..8].to_string();
            let existing = self.tasks_by_project.get(&project_id).map(|t| t.iter().any(|task| task.id == candidate)).unwrap_or(false);
            if !existing {
                break candidate;
            }
        };
        let now = Utc::now();

        let task_dir = self.store.task_dir(&project_id, &task_id);
        std::fs::create_dir_all(&task_dir)?;

        // Create worktrees from project repos
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

        let repos: Vec<(String, std::path::PathBuf)> = project
            .repos
            .iter()
            .map(|r| (r.name.clone(), r.path.clone()))
            .collect();

        let worktrees = if !repos.is_empty() {
            match self
                .worktree_svc
                .create_worktrees_for_task(&task_dir, &task_id, &repos)
            {
                Ok(wts) => wts,
                Err(_) => Vec::new(), // Worktree creation failed, continue without
            }
        } else {
            Vec::new()
        };

        // Create tmux session
        let session_name = TmuxService::session_name(&project_id, &task_id);
        let tmux_session = if TmuxService::is_available() {
            self.tmux.create_session(&session_name, &task_dir)?;
            if agent_cli != AgentCli::None {
                self.tmux.launch_agent(&session_name, &agent_cli)?;
            }
            Some(session_name)
        } else {
            None
        };

        let task = Task {
            id: task_id,
            project_id,
            name,
            priority,
            status: crate::domain::task::Status::Todo,
            agent_cli,
            worktrees,
            links: Vec::new(),
            notes,
            tmux_session: tmux_session.clone(),
            created_at: now,
            updated_at: now,
        };

        if let Err(e) = self.store.save_task(&task).and_then(|_| self.store.write_agent_config_files(&task)) {
            // Rollback: kill tmux session and remove worktrees
            if let Some(session) = &tmux_session {
                let _ = self.tmux.kill_session(session);
            }
            for wt in &task.worktrees {
                let _ = self.worktree_svc.remove_worktree(wt);
            }
            let _ = std::fs::remove_dir_all(&task_dir);
            return Err(e);
        }

        Ok(())
    }

    /// Returns Some(session_name) if we should attach to a tmux session,
    /// None otherwise (e.g., toggled expand on project, or no session).
    fn resolve_attach_session(&mut self) -> Option<String> {
        let item = self.task_tree.selected_item().cloned()?;

        match item {
            TreeItem::Project { .. } => {
                self.task_tree.toggle_expand();
                self.rebuild_tree();
                None
            }
            TreeItem::Task {
                id,
                project_id,
                has_session,
                ..
            } => {
                if !has_session {
                    return None;
                }
                let session_name = self
                    .tasks_by_project
                    .get(&project_id)
                    .and_then(|tasks| tasks.iter().find(|t| t.id == id))
                    .and_then(|t| t.tmux_session.clone());

                session_name.filter(|name| self.tmux.session_exists(name))
            }
        }
    }

    fn cleanup_task(&self, task: &Task) -> AppResult<()> {
        // Kill tmux session
        if let Some(session) = &task.tmux_session {
            self.tmux.kill_session(session)?;
        }
        // Remove worktrees
        for wt in &task.worktrees {
            let _ = self.worktree_svc.remove_worktree(wt);
        }
        Ok(())
    }

    fn update_task<F>(&self, project_id: &str, task_id: &str, updater: F) -> AppResult<()>
    where
        F: FnOnce(&mut Task),
    {
        let task = self
            .tasks_by_project
            .get(project_id)
            .and_then(|tasks| tasks.iter().find(|t| t.id == task_id))
            .ok_or_else(|| anyhow::anyhow!("Task {} not found in project {}", task_id, project_id))?;

        let mut updated = task.clone();
        updater(&mut updated);
        self.store.save_task(&updated)?;
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let main_chunks = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

        let content_chunks = Layout::horizontal([
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(main_chunks[0]);

        // Left panel: task tree
        self.task_tree.render(frame, content_chunks[0]);

        // Right panel: preview
        self.preview_panel.render(frame, content_chunks[1]);

        // Status bar
        if self.active_modal.is_some() {
            StatusBar::render_modal(frame, main_chunks[1]);
        } else {
            StatusBar::render_main(frame, main_chunks[1], self.error_message.as_deref());
        }

        // Modal overlay
        if let Some(modal) = &self.active_modal {
            let modal_area = centered_rect(60, 70, area);
            match modal {
                ModalKind::CreateProject(m) => m.render(frame, modal_area),
                ModalKind::CreateTask(m) => m.render(frame, modal_area),
                ModalKind::EditItem(m) => m.render(frame, modal_area),
                ModalKind::SetStatus(m) => m.render(frame, centered_rect(40, 40, area)),
                ModalKind::SetLink(m) => m.render(frame, centered_rect(50, 30, area)),
                ModalKind::SelectLink(m) => m.render(frame, centered_rect(50, 40, area)),
                ModalKind::Filter(m) => m.render(frame, centered_rect(40, 40, area)),
                ModalKind::Sort(m) => m.render(frame, centered_rect(40, 30, area)),
                ModalKind::ConfirmDelete(m) => m.render(frame, centered_rect(50, 35, area)),
            }
        }
    }
}
