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
use crate::components::modals::{centered_rect, centered_rect_with_max, Modal};
use crate::components::preview_panel::PreviewPanel;
use crate::components::status_bar::StatusBar;
use crate::components::task_tree::{TaskTree, TreeItem};
use crate::config::Config;
use crate::domain::project::{Project, RepoRef};
use crate::domain::task::{AgentCli, Priority, Status, Task, TaskLink};
use crate::error::AppResult;
use crate::services::agent_monitor::AgentMonitor;
use crate::services::git_finder;
use crate::services::pr_monitor::PrMonitor;
use crate::services::tmux::TmuxService;
use crate::services::worktree::WorktreeService;
use crate::storage::FsStore;
use anyhow::Context;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
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
    repo_scan_rx: Option<mpsc::Receiver<Vec<std::path::PathBuf>>>,
    pub active_sessions: HashSet<String>,

    // Background task setup receiver
    task_setup_rx: Vec<mpsc::Receiver<TaskSetupResult>>,
    // Background error messages (e.g. from initial instructions sending)
    bg_error_rx: Vec<mpsc::Receiver<String>>,

    // Monitors
    agent_monitor: AgentMonitor,
    pr_monitor: PrMonitor,
    tick_count: u64,

    // Error display
    pub error_message: Option<String>,
}

/// Result from a background task setup (worktree + tmux + config).
struct TaskSetupResult {
    task_id: String,
    project_id: String,
    worktrees: Vec<crate::domain::task::WorktreeInfo>,
    tmux_session: Option<String>,
    error: Option<String>,
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
        let default_sort_mode = config.default_sort_mode;
        let store = FsStore::new(&config)?;
        store.ensure_quickstart()?;
        let tmux = TmuxService::new();
        let worktree_svc = WorktreeService::new();
        let agent_monitor = AgentMonitor::new(store.clone(), TmuxService::new());
        let pr_monitor = PrMonitor::new(store.clone());

        // Start background git repo scan immediately
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let repos = git_finder::find_git_repos().unwrap_or_default();
            let _ = tx.send(repos);
        });

        let mut app = Self {
            running: true,
            config,
            store,
            tmux,
            worktree_svc,
            projects: Vec::new(),
            tasks_by_project: HashMap::new(),
            task_tree: TaskTree::new(default_sort_mode),
            preview_panel: PreviewPanel::new(),
            active_modal: None,
            available_repos: Vec::new(),
            repo_scan_rx: Some(rx),
            active_sessions: HashSet::new(),
            task_setup_rx: Vec::new(),
            bg_error_rx: Vec::new(),
            agent_monitor,
            pr_monitor,
            tick_count: 0,
            error_message: None,
        };

        app.reload_data()?;

        // Re-generate Claude Code hooks for all existing Claude tasks
        // to ensure latest hook configuration (e.g. .manual_todo cleanup).
        for tasks in app.tasks_by_project.values() {
            for task in tasks {
                if task.agent_cli == AgentCli::Claude {
                    let _ = app.store.write_claude_hooks(task);
                }
            }
        }

        // Expand all projects by default
        for p in &app.projects {
            app.task_tree.expanded.insert(p.id.clone());
        }
        app.rebuild_tree();

        Ok(app)
    }

    /// Poll for completed background task setup results and apply them.
    fn poll_task_setup_results(&mut self) {
        let mut completed = Vec::new();
        let mut i = 0;
        while i < self.task_setup_rx.len() {
            match self.task_setup_rx[i].try_recv() {
                Ok(result) => {
                    completed.push(result);
                    self.task_setup_rx.swap_remove(i);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.task_setup_rx.swap_remove(i);
                }
            }
        }

        for result in completed {
            if let Some(error) = result.error {
                self.error_message = Some(format!("Task setup error: {}", error));
            }
            let _ = self.update_task(&result.project_id, &result.task_id, |task| {
                task.worktrees = result.worktrees;
                task.tmux_session = result.tmux_session;
                task.updated_at = Utc::now();
            });
            let _ = self.reload_data();
            self.rebuild_tree();
            self.refresh_preview_task_info();
        }

        // Poll background error messages (e.g. from initial instructions sending)
        let mut i = 0;
        while i < self.bg_error_rx.len() {
            match self.bg_error_rx[i].try_recv() {
                Ok(error) => {
                    self.error_message = Some(error);
                    self.bg_error_rx.swap_remove(i);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.bg_error_rx.swap_remove(i);
                }
            }
        }
    }

    /// Check if the background repo scan has completed and store results.
    fn try_receive_repos(&mut self) {
        if let Some(rx) = self.repo_scan_rx.take() {
            match rx.try_recv() {
                Ok(repos) => {
                    self.available_repos = repos;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Not ready yet, put the receiver back
                    self.repo_scan_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Thread finished without sending (shouldn't happen)
                }
            }
        }
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

    pub fn handle_paste_event(&mut self, text: &str) {
        if let Some(modal) = &mut self.active_modal {
            match modal {
                ModalKind::CreateProject(m) => m.handle_paste(text),
                ModalKind::CreateTask(m) => m.handle_paste(text),
                ModalKind::EditItem(m) => m.handle_paste(text),
                ModalKind::SetLink(m) => m.handle_paste(text),
                ModalKind::SetStatus(m) => m.handle_paste(text),
                ModalKind::SelectLink(m) => m.handle_paste(text),
                ModalKind::Filter(m) => m.handle_paste(text),
                ModalKind::Sort(m) => m.handle_paste(text),
                ModalKind::ConfirmDelete(m) => m.handle_paste(text),
            }
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // Remap Ctrl+N/P to arrow keys, Ctrl+F/B/A/E to cursor movement, Ctrl+H/D to delete
        let key = if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('n') => KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
                KeyCode::Char('p') => KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                KeyCode::Char('f') => KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                KeyCode::Char('b') => KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                KeyCode::Char('a') => KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
                KeyCode::Char('e') => KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
                KeyCode::Char('h') => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                KeyCode::Char('d') => KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
                _ => key,
            }
        } else {
            key
        };

        // Modal takes priority
        if let Some(modal) = &mut self.active_modal {
            // Ctrl+C acts as Escape to close modals
            if key.code == KeyCode::Char('c')
                && key.modifiers == KeyModifiers::CONTROL
            {
                return Ok(Some(Action::CloseModal));
            }
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
            KeyCode::Char('A') => return Ok(Some(Action::FilterActionRequired)),
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
            KeyCode::Char(c @ '1'..='5') => {
                if let Some(TreeItem::Task { id, project_id, .. }) =
                    self.task_tree.selected_item()
                {
                    let priority = match c {
                        '1' => Priority::P1,
                        '2' => Priority::P2,
                        '3' => Priority::P3,
                        '4' => Priority::P4,
                        _ => Priority::P5,
                    };
                    return Ok(Some(Action::UpdateTaskPriority {
                        task_id: id.clone(),
                        project_id: project_id.clone(),
                        priority,
                    }));
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
            Action::AttachSession => {
                if let Some(name) = self.resolve_attach_session() {
                    return Ok(UpdateResult::AttachSession(name));
                }
            }

            // Modal openers
            Action::OpenCreateProject => {
                self.try_receive_repos();
                if self.available_repos.is_empty() {
                    // Fallback: scan synchronously if background scan hasn't completed
                    self.available_repos = git_finder::find_git_repos().unwrap_or_default();
                }
                self.active_modal = Some(ModalKind::CreateProject(CreateProjectModal::new(
                    self.available_repos.clone(),
                )));
            }
            Action::OpenCreateTask => {
                if self.projects.is_empty() {
                    // No projects yet - prompt to create one first
                    self.try_receive_repos();
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
                            let current_copy_files: Vec<String> = project
                                .map(|p| p.worktree_copy_files.clone())
                                .unwrap_or_default();

                            self.active_modal = Some(ModalKind::EditItem(EditItemModal::Project(
                                EditProjectModal::new(
                                    id,
                                    name,
                                    self.available_repos.clone(),
                                    selected_repos,
                                    current_copy_files,
                                ),
                            )));
                        }
                        TreeItem::Task {
                            id,
                            project_id,
                            name,
                            priority,
                            notes,
                            ..
                        } => {
                            self.active_modal = Some(ModalKind::EditItem(EditItemModal::Task(
                                EditTaskModal::new(id, project_id, name, priority, notes),
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
            Action::FilterActionRequired => {
                // Toggle: if already filtering ActionRequired only, clear filter
                let is_already = matches!(
                    &self.task_tree.status_filter,
                    Some(f) if f.len() == 1 && f[0] == Status::ActionRequired
                );
                if is_already {
                    self.task_tree.status_filter = None;
                } else {
                    self.task_tree.status_filter = Some(vec![Status::ActionRequired]);
                }
                self.rebuild_tree();
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
            Action::CreateProject { name, repos, worktree_copy_files } => {
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
                    worktree_copy_files,
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
                worktree_copy_files,
            } => {
                if let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() {
                    let mut updated = project;
                    updated.name = name;
                    updated.repos = repos
                        .into_iter()
                        .map(|(n, p)| RepoRef { name: n, path: p })
                        .collect();
                    updated.worktree_copy_files = worktree_copy_files;
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
                links,
                initial_instructions,
            } => {
                let task_id = self.handle_create_task(project_id.clone(), name, priority, agent_cli, notes, links, initial_instructions)?;
                self.active_modal = None;
                self.reload_data()?;
                self.task_tree.expanded.insert(project_id);
                self.rebuild_tree();
                self.task_tree.select_task_by_id(&task_id);
                self.refresh_preview_task_info();
            }
            Action::UpdateTask {
                task_id,
                project_id,
                name,
                priority,
                notes,
            } => {
                self.update_task(&project_id, &task_id, |task| {
                    task.name = name;
                    task.priority = priority;
                    task.notes = notes;
                    task.updated_at = Utc::now();
                })?;
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
                self.refresh_preview_task_info();
            }
            Action::UpdateTaskPriority {
                task_id,
                project_id,
                priority,
            } => {
                self.update_task(&project_id, &task_id, |task| {
                    task.priority = priority;
                    task.updated_at = Utc::now();
                })?;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::UpdateTaskStatus {
                task_id,
                project_id,
                status,
            } => {
                // Sync manual_todo marker with manual status changes on Claude tasks.
                // When manually setting Todo, write marker so the monitor
                // won't flip to InProgress until PreToolUse clears it.
                if let Some(tasks) = self.tasks_by_project.get(&project_id) {
                    if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
                        if task.agent_cli == AgentCli::Claude {
                            let task_dir = self.store.task_dir(&project_id, &task_id);
                            let manual_todo_path = task_dir.join(".manual_todo");
                            let todo_res = if status == Status::Todo {
                                std::fs::write(&manual_todo_path, "manual\n")
                            } else {
                                std::fs::remove_file(&manual_todo_path).or_else(|e| {
                                    if e.kind() == std::io::ErrorKind::NotFound {
                                        Ok(())
                                    } else {
                                        Err(e)
                                    }
                                })
                            };
                            if let Err(e) = todo_res {
                                self.error_message = Some(format!("Manual todo marker sync failed: {}", e));
                            }
                        }
                    }
                }
                self.update_task(&project_id, &task_id, |task| {
                    // When reopening a Completed task, record reopened_at so
                    // PrMonitor won't auto-complete from already-merged PRs.
                    if task.status == Status::Completed && status != Status::Completed {
                        task.reopened_at = Some(Utc::now());
                    }
                    // Clear reopened_at when manually completing
                    if status == Status::Completed {
                        task.reopened_at = None;
                    }
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
                self.refresh_preview_task_info();
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

            Action::Tick => {
                self.tick_count += 1;

                // Poll background task setup results
                self.poll_task_setup_results();

                // Refresh active sessions periodically
                self.active_sessions.clear();
                if let Ok(sessions) = self.tmux.list_sessions() {
                    for s in sessions {
                        self.active_sessions.insert(s);
                    }
                }

                // Update preview with task info
                self.refresh_preview_task_info();

                let session_name = self.task_tree.selected_item().and_then(|item| {
                    if let TreeItem::Task { id, project_id, .. } = item {
                        self.tasks_by_project
                            .get(project_id.as_str())
                            .and_then(|tasks| tasks.iter().find(|t| t.id == *id))
                            .and_then(|t| t.tmux_session.as_deref())
                    } else {
                        None
                    }
                });
                let session_name_owned = session_name.map(|s| s.to_string());
                self.preview_panel
                    .update_preview(session_name_owned.as_deref(), &self.tmux);

                // Agent monitor: check every monitor_interval_secs
                // tick_rate_ms=250 → 4 ticks/sec → interval_secs * 4 ticks
                let agent_interval_ticks =
                    (self.config.monitor_interval_secs * 1000 / self.config.tick_rate_ms).max(1);
                let mut data_changed = false;

                if self.tick_count % agent_interval_ticks == 0 {
                    let agent_events = self.agent_monitor.check_all();
                    for event in agent_events {
                        match event {
                            crate::services::agent_monitor::MonitorEvent::StatusChanged {
                                task_id,
                                project_id,
                                status,
                            } => {
                                let _ = self.update_task(&project_id, &task_id, |task| {
                                    task.status = status;
                                    task.updated_at = Utc::now();
                                });
                                data_changed = true;
                            }
                            crate::services::agent_monitor::MonitorEvent::PrLinkDiscovered {
                                task_id,
                                project_id,
                                url,
                            } => {
                                let link = TaskLink { url, display_name: None };
                                let _ = self.update_task(&project_id, &task_id, |task| {
                                    task.links.push(link);
                                    task.updated_at = Utc::now();
                                });
                                data_changed = true;
                            }
                        }
                    }
                }

                // PR monitor: kick off background check every pr_monitor_interval_secs
                let pr_interval_ticks =
                    (self.config.pr_monitor_interval_secs * 1000 / self.config.tick_rate_ms).max(1);
                if self.tick_count % pr_interval_ticks == 0 {
                    self.pr_monitor.start_check();
                }

                // Poll for completed PR check results (non-blocking)
                let pr_events = self.pr_monitor.poll_results();
                for event in pr_events {
                    match event {
                        crate::services::pr_monitor::PrMonitorEvent::AllPrsMerged {
                            task_id,
                            project_id,
                        } => {
                            let _ = self.update_task(&project_id, &task_id, |task| {
                                task.status = Status::Completed;
                                task.reopened_at = None;
                                task.updated_at = Utc::now();
                            });
                            data_changed = true;
                        }
                    }
                }

                if data_changed {
                    self.reload_data()?;
                    self.rebuild_tree();
                }
            }

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
        links: Vec<crate::domain::task::TaskLink>,
        initial_instructions: Option<String>,
    ) -> AppResult<String> {
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

        // Save task immediately (without worktrees/tmux) so UI updates fast
        let task = Task {
            id: task_id.clone(),
            project_id: project_id.clone(),
            name,
            priority,
            status: crate::domain::task::Status::Todo,
            agent_cli,
            worktrees: Vec::new(),
            links,
            notes,
            initial_instructions: initial_instructions.clone(),
            tmux_session: None,
            created_at: now,
            updated_at: now,
            reopened_at: None,
        };

        self.store.save_task(&task)?;

        // Write .manual_todo marker so the monitor keeps the task in Todo
        // until the agent actually starts executing tools (PreToolUse clears it).
        if task.agent_cli == AgentCli::Claude {
            let _ = std::fs::write(task_dir.join(".manual_todo"), "manual\n");
        }

        // Find project info for background setup
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

        // Spawn background thread for heavy operations (worktree, tmux, config files)
        let store = self.store.clone();
        let worktree_svc = WorktreeService::new();
        let tmux = TmuxService::new();
        let (tx, rx) = mpsc::channel();
        let (err_tx, err_rx) = mpsc::channel();
        let bg_task = task.clone();
        let bg_task_dir = task_dir.clone();
        let bg_initial_instructions = initial_instructions;

        std::thread::spawn(move || {
            let repos: Vec<(String, std::path::PathBuf)> = project
                .repos
                .iter()
                .map(|r| (r.name.clone(), r.path.clone()))
                .collect();

            // Create worktrees
            let worktrees = if !repos.is_empty() {
                match worktree_svc.create_worktrees_for_task(&bg_task_dir, &bg_task.id, &repos) {
                    Ok(wts) => {
                        if !project.worktree_copy_files.is_empty() {
                            for wt in &wts {
                                let _ = WorktreeService::copy_files_to_worktree(
                                    &wt.upstream_path,
                                    &wt.worktree_path,
                                    &project.worktree_copy_files,
                                );
                            }
                        }
                        wts
                    }
                    Err(_) => Vec::new(),
                }
            } else {
                Vec::new()
            };

            // Build initial prompt file if instructions were provided
            let prompt_file = if let Some(instructions) = bg_initial_instructions {
                if bg_task.agent_cli != AgentCli::None {
                    let mut prompt = instructions;
                    let link_urls: Vec<String> = bg_task.links.iter().map(|l| l.url.clone()).collect();
                    if !link_urls.is_empty() {
                        prompt.push_str("\n\nLinks:\n");
                        for url in &link_urls {
                            prompt.push_str(&format!("- {}\n", url));
                        }
                    }
                    let path = bg_task_dir.join(".initial_prompt");
                    match std::fs::write(&path, &prompt) {
                        Ok(()) => Some(path),
                        Err(e) => {
                            let _ = err_tx.send(format!("Failed to write initial prompt file: {}", e));
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Create tmux session
            let session_name = TmuxService::session_name(&bg_task.project_id, &bg_task.id);
            let tmux_session = if TmuxService::is_available() {
                match tmux.create_session(&session_name, &bg_task_dir) {
                    Ok(()) => {
                        if bg_task.agent_cli != AgentCli::None {
                            let _ = tmux.launch_agent(
                                &session_name,
                                &bg_task.agent_cli,
                                prompt_file.as_deref(),
                            );
                        }
                        Some(session_name)
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            // Write agent config files (needs updated task with worktrees)
            let mut updated_task = bg_task;
            updated_task.worktrees = worktrees.clone();
            updated_task.tmux_session = tmux_session.clone();
            let error = match store.save_task(&updated_task).and_then(|_| store.write_agent_config_files(&updated_task)) {
                Ok(()) => None,
                Err(e) => Some(format!("{}", e)),
            };

            // Send TaskSetupResult immediately so the UI updates
            let _ = tx.send(TaskSetupResult {
                task_id: updated_task.id.clone(),
                project_id: updated_task.project_id.clone(),
                worktrees,
                tmux_session: tmux_session.clone(),
                error,
            });
        });

        self.task_setup_rx.push(rx);
        self.bg_error_rx.push(err_rx);

        Ok(task_id)
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
                status,
                ..
            } => {
                let task = self
                    .tasks_by_project
                    .get(&project_id)
                    .and_then(|tasks| tasks.iter().find(|t| t.id == id))
                    .cloned();
                let task = task?;

                let session_name = task
                    .tmux_session
                    .clone()
                    .unwrap_or_else(|| TmuxService::session_name(&project_id, &id));

                // Helper closure: reopen Completed task as InProgress
                let maybe_reopen = |app: &mut Self| {
                    if status == Status::Completed {
                        let _ = app.update_task(&project_id, &id, |task| {
                            task.reopened_at = Some(Utc::now());
                            task.status = Status::InProgress;
                            task.updated_at = Utc::now();
                        });
                    }
                };

                // Session already exists – just attach
                if self.tmux.session_exists(&session_name) {
                    maybe_reopen(self);
                    return Some(session_name);
                }

                // No active session – recreate it
                if !TmuxService::is_available() {
                    return None;
                }
                let task_dir = self.store.task_dir(&project_id, &id);
                if !task_dir.exists() {
                    return None;
                }
                match self.tmux.create_session(&session_name, &task_dir) {
                    Ok(()) => {
                        // Launch agent if configured
                        if task.agent_cli != AgentCli::None {
                            let prompt_file = task_dir.join(".initial_prompt");
                            let prompt_path = if prompt_file.exists() {
                                Some(prompt_file.as_path())
                            } else {
                                None
                            };
                            let _ = self.tmux.launch_agent(
                                &session_name,
                                &task.agent_cli,
                                prompt_path,
                            );
                        }
                        // Persist the session name if it wasn't stored yet
                        if task.tmux_session.is_none() {
                            if let Some(tasks) = self.tasks_by_project.get_mut(&project_id) {
                                if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
                                    t.tmux_session = Some(session_name.clone());
                                    if let Err(e) = self.store.save_task(t) {
                                        self.error_message = Some(format!("Failed to save task: {}", e));
                                    }
                                }
                            }
                        }
                        self.active_sessions.insert(session_name.clone());
                        self.rebuild_tree();
                        maybe_reopen(self);
                        Some(session_name)
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to create tmux session: {}", e));
                        None
                    }
                }
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
        // Read the latest state from disk to avoid last-write-wins when
        // multiple events target the same task in a single tick.
        let task_dir = self.store.task_dir(project_id, task_id);
        let json_path = task_dir.join("task.json");
        let content = std::fs::read_to_string(&json_path)
            .with_context(|| format!("reading {:?}", json_path))?;
        let mut updated: Task = serde_json::from_str(&content)
            .with_context(|| format!("parsing {:?}", json_path))?;

        updater(&mut updated);
        self.store.save_task(&updated)?;
        Ok(())
    }

    fn refresh_preview_task_info(&mut self) {
        let selected = self.task_tree.selected_item();

        match selected {
            Some(TreeItem::Task { id, project_id, .. }) => {
                let task = self
                    .tasks_by_project
                    .get(project_id.as_str())
                    .and_then(|tasks| tasks.iter().find(|t| t.id == *id));
                if let Some(task) = task {
                    self.preview_panel
                        .update_task_info(task.links.clone(), task.notes.clone(), task.initial_instructions.clone());
                } else {
                    self.preview_panel.update_task_info(Vec::new(), None, None);
                }
            }
            Some(TreeItem::Project { id, name, .. }) => {
                let project = self.projects.iter().find(|p| p.id == *id);
                let tasks = self.tasks_by_project.get(id.as_str());

                let mut stats = crate::components::preview_panel::TaskStats::default();
                if let Some(tasks) = tasks {
                    stats.total = tasks.len();
                    for t in tasks {
                        match t.status {
                            crate::domain::task::Status::Todo => stats.todo += 1,
                            crate::domain::task::Status::InProgress => stats.in_progress += 1,
                            crate::domain::task::Status::ActionRequired => stats.action_required += 1,
                            crate::domain::task::Status::Completed => stats.completed += 1,
                            crate::domain::task::Status::Blocked => stats.blocked += 1,
                        }
                    }
                }

                let repos = project
                    .map(|p| {
                        p.repos
                            .iter()
                            .map(|r| crate::components::preview_panel::RepoInfo {
                                name: r.name.clone(),
                                path: r.path.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let worktree_copy_files = project
                    .map(|p| p.worktree_copy_files.clone())
                    .unwrap_or_default();

                self.preview_panel
                    .update_project_info(crate::components::preview_panel::ProjectInfo {
                        name: name.clone(),
                        repos,
                        worktree_copy_files,
                        task_stats: stats,
                    });
            }
            None => {
                self.preview_panel.update_task_info(Vec::new(), None, None);
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let main_chunks = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

        let content_chunks = Layout::horizontal([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
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
                ModalKind::CreateTask(m) => m.render(frame, centered_rect_with_max(90, 90, 120, 55, area)),
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
