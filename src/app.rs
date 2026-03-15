use crate::action::Action;
use crate::components::modals::confirm_delete::{ConfirmDeleteModal, DeleteTarget};
use crate::components::modals::create_project::CreateProjectModal;
use crate::components::modals::create_task::CreateTaskModal;
use crate::components::modals::custom_prompt::CustomPromptModal;
use crate::components::modals::edit_item::{EditItemModal, EditProjectModal, EditTaskModal};
use crate::components::modals::filter::FilterModal;
use crate::components::modals::select_link::SelectLinkModal;
use crate::components::modals::select_preview_url::SelectPreviewUrlModal;
use crate::components::modals::set_link::SetLinkModal;
use crate::components::modals::set_status::SetStatusModal;
use crate::components::modals::settings::SettingsModal;
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
use crate::services::pm_scheduler::{PmScheduler, PmSchedulerEvent};
use crate::services::pr_monitor::PrMonitor;
use crate::services::task_setup::{self, write_initial_prompt, TaskSetupInput};
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

    // Monitors
    agent_monitor: AgentMonitor,
    pr_monitor: PrMonitor,
    pm_scheduler: PmScheduler,
    tick_count: u64,

    // Filesystem change detection
    last_data_fingerprint: (u128, usize),

    // Flag to request a full terminal redraw (resets ratatui front buffer).
    // Used to recover from any external writes that bypass ratatui's buffer.
    pub needs_full_redraw: bool,

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
    SelectPreviewUrl(SelectPreviewUrlModal),
    Filter(FilterModal),
    Sort(SortModal),
    ConfirmDelete(ConfirmDeleteModal),
    Settings(SettingsModal),
    CustomPrompt(CustomPromptModal),
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
        let pm_scheduler = PmScheduler::new(store.clone());

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
            agent_monitor,
            pr_monitor,
            pm_scheduler,
            tick_count: 0,
            last_data_fingerprint: (0, 0),
            needs_full_redraw: false,
            error_message: None,
        };

        app.reload_data()?;
        app.last_data_fingerprint = app.store.data_fingerprint();

        // Re-generate hooks for all existing agent tasks
        // to ensure latest hook configuration.
        for tasks in app.tasks_by_project.values() {
            for task in tasks {
                match task.agent_cli {
                    AgentCli::Claude => { let _ = app.store.write_claude_hooks(task); }
                    AgentCli::Gemini => { let _ = app.store.write_gemini_hooks(task); }
                    _ => {}
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

        if !completed.is_empty() {
            // Background task setup threads may have written to the terminal
            // (e.g. via inherited stdio from subprocess commands), so request
            // a full redraw to recover from any display corruption.
            self.needs_full_redraw = true;
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
        self.task_tree.rebuild(
            &self.projects,
            &self.tasks_by_project,
            &self.active_sessions,
        );
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
                ModalKind::SelectPreviewUrl(m) => m.handle_paste(text),
                ModalKind::Filter(m) => m.handle_paste(text),
                ModalKind::Sort(m) => m.handle_paste(text),
                ModalKind::ConfirmDelete(m) => m.handle_paste(text),
                ModalKind::Settings(m) => m.handle_paste(text),
                ModalKind::CustomPrompt(m) => m.handle_paste(text),
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
            if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                return Ok(Some(Action::CloseModal));
            }
            return match modal {
                ModalKind::CreateProject(m) => m.handle_key(key),
                ModalKind::CreateTask(m) => m.handle_key(key),
                ModalKind::EditItem(m) => m.handle_key(key),
                ModalKind::SetStatus(m) => m.handle_key(key),
                ModalKind::SetLink(m) => m.handle_key(key),
                ModalKind::SelectLink(m) => m.handle_key(key),
                ModalKind::SelectPreviewUrl(m) => m.handle_key(key),
                ModalKind::Filter(m) => m.handle_key(key),
                ModalKind::Sort(m) => m.handle_key(key),
                ModalKind::ConfirmDelete(m) => m.handle_key(key),
                ModalKind::Settings(m) => m.handle_key(key),
                ModalKind::CustomPrompt(m) => m.handle_key(key),
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
            KeyCode::Char('C') => {
                return Ok(Some(Action::OpenSettings));
            }
            KeyCode::Char('P') => {
                if let Some(TreeItem::Task { id, project_id, .. }) = self.task_tree.selected_item()
                {
                    return Ok(Some(Action::SendPrInstruction {
                        task_id: id.clone(),
                        project_id: project_id.clone(),
                    }));
                }
            }
            KeyCode::Char('L') => {
                return Ok(Some(Action::OpenSetLink));
            }
            KeyCode::Char('o') => {
                match self.task_tree.selected_item() {
                    // Task selected: open link(s) in external browser
                    Some(TreeItem::Task { id, project_id, .. }) => {
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
                    // Project selected: toggle task list expand/collapse
                    Some(TreeItem::Project { .. }) => {
                        self.task_tree.toggle_expand();
                        self.rebuild_tree();
                    }
                    None => {}
                }
            }
            KeyCode::Char(c @ '1'..='5') => {
                if let Some(TreeItem::Task { id, project_id, .. }) = self.task_tree.selected_item()
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
            KeyCode::Char('R') => {
                if let Some(TreeItem::Task { id, project_id, .. }) = self.task_tree.selected_item()
                {
                    return Ok(Some(Action::SendReviewInstruction {
                        task_id: id.clone(),
                        project_id: project_id.clone(),
                    }));
                }
            }
            KeyCode::Char('v') => {
                return Ok(Some(Action::OpenPreviewUrl));
            }
            KeyCode::Char('U') => {
                return Ok(Some(Action::OpenCustomPrompt));
            }
            KeyCode::Char('M') => {
                if let Some(item) = self.task_tree.selected_item() {
                    let project_id = item.project_id().to_string();
                    return Ok(Some(Action::StartPmSession { project_id }));
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
                            let current_description: Option<String> =
                                project.and_then(|p| p.description.clone());
                            let current_copy_files: Vec<String> = project
                                .map(|p| p.worktree_copy_files.clone())
                                .unwrap_or_default();
                            let current_dev_env_prompt: Option<String> =
                                project.and_then(|p| p.dev_environment_prompt.clone());
                            let current_pm_enabled =
                                project.map(|p| p.pm_enabled).unwrap_or(false);
                            let current_pm_agent_cli =
                                project.and_then(|p| p.pm_agent_cli);
                            let current_pm_cron: Option<String> =
                                project.and_then(|p| p.pm_cron_expression.clone());
                            let current_pm_custom_instructions: Option<String> =
                                project.and_then(|p| p.pm_custom_instructions.clone());

                            self.active_modal = Some(ModalKind::EditItem(EditItemModal::Project(
                                EditProjectModal::new(
                                    id,
                                    name,
                                    current_description,
                                    self.available_repos.clone(),
                                    selected_repos,
                                    current_copy_files,
                                    current_dev_env_prompt,
                                    current_pm_enabled,
                                    current_pm_agent_cli,
                                    current_pm_cron,
                                    current_pm_custom_instructions,
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
                    self.active_modal = Some(ModalKind::SetLink(SetLinkModal::new(id, project_id)));
                }
            }
            Action::OpenPreviewUrl => {
                if let Some(TreeItem::Task { id, project_id, .. }) =
                    self.task_tree.selected_item()
                {
                    if let Some(tasks) = self.tasks_by_project.get(project_id.as_str()) {
                        if let Some(task) = tasks.iter().find(|t| t.id == *id) {
                            match task.preview_urls.len() {
                                0 => {}
                                1 => {
                                    return self.update(Action::OpenLinkInBrowser {
                                        url: task.preview_urls[0].url.clone(),
                                    });
                                }
                                _ => {
                                    self.active_modal =
                                        Some(ModalKind::SelectPreviewUrl(
                                            SelectPreviewUrlModal::new(
                                                task.preview_urls.clone(),
                                            ),
                                        ));
                                }
                            }
                        }
                    }
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
                self.active_modal = Some(ModalKind::Sort(SortModal::new(self.task_tree.sort_mode)));
            }
            Action::OpenConfirmDelete => {
                if let Some(item) = self.task_tree.selected_item().cloned() {
                    let target = match item {
                        TreeItem::Project { id, name, .. } => DeleteTarget::Project { id, name },
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

            Action::OpenSettings => {
                self.active_modal = Some(ModalKind::Settings(SettingsModal::new(&self.config)));
            }

            Action::SaveSettings {
                pr_prompt,
                review_prompt,
            } => {
                let mut new_config = self.config.clone();
                new_config.pr_prompt = pr_prompt;
                new_config.review_prompt = review_prompt;
                match new_config.save() {
                    Ok(()) => {
                        self.config = new_config;
                        self.active_modal = None;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to save settings: {}", e));
                    }
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
            Action::CreateProject {
                name,
                description,
                repos,
                worktree_copy_files,
                dev_environment_prompt,
                pm_enabled,
                pm_agent_cli,
                pm_custom_instructions,
                pm_cron_expression,
            } => {
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
                    description,
                    worktree_copy_files,
                    dev_environment_prompt,
                    pm_enabled,
                    pm_agent_cli,
                    pm_custom_instructions,
                    pm_cron_expression,
                    pm_tmux_session: None,
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
                description,
                repos,
                worktree_copy_files,
                dev_environment_prompt,
                pm_enabled,
                pm_agent_cli,
                pm_custom_instructions,
                pm_cron_expression,
            } => {
                if let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() {
                    let mut updated = project;
                    updated.name = name;
                    updated.description = description;
                    updated.repos = repos
                        .into_iter()
                        .map(|(n, p)| RepoRef { name: n, path: p })
                        .collect();
                    updated.worktree_copy_files = worktree_copy_files;
                    updated.dev_environment_prompt = dev_environment_prompt;
                    // If PM was disabled, kill session
                    if updated.pm_enabled && !pm_enabled {
                        let pm_session = TmuxService::pm_session_name(&project_id);
                        let _ = self.tmux.kill_session(&pm_session);
                        updated.pm_tmux_session = None;
                    }
                    updated.pm_enabled = pm_enabled;
                    updated.pm_agent_cli = pm_agent_cli;
                    updated.pm_custom_instructions = pm_custom_instructions;
                    updated.pm_cron_expression = pm_cron_expression;
                    updated.updated_at = Utc::now();
                    self.store.save_project(&updated)?;
                }
                self.active_modal = None;
                self.reload_data()?;
                self.rebuild_tree();
            }
            Action::DeleteProject { project_id } => {
                // Kill PM session if active
                let pm_session = TmuxService::pm_session_name(&project_id);
                let _ = self.tmux.kill_session(&pm_session);
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
                let task_id = self.handle_create_task(
                    project_id.clone(),
                    name,
                    priority,
                    agent_cli,
                    notes,
                    links,
                    initial_instructions,
                )?;
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
                // Clear marker files when manually changing status,
                // so the monitor won't override the manual change.
                if let Some(tasks) = self.tasks_by_project.get(&project_id) {
                    if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
                        if matches!(task.agent_cli, AgentCli::Claude | AgentCli::Codex | AgentCli::Gemini) {
                            let task_dir = self.store.task_dir(&project_id, &task_id);
                            let _ = std::fs::remove_file(task_dir.join(".prompt_submitted"));
                            let _ = std::fs::remove_file(task_dir.join(".agent_stopped"));
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
                    if !task.links.iter().any(|l| l.url == link.url) {
                        task.links.push(link);
                        task.updated_at = Utc::now();
                    }
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

            Action::SendPrInstruction {
                task_id,
                project_id,
            } => {
                let task = self
                    .tasks_by_project
                    .get(project_id.as_str())
                    .and_then(|tasks| tasks.iter().find(|t| t.id == task_id))
                    .cloned();
                if let Some(task) = task {
                    let session_name = task
                        .tmux_session
                        .clone()
                        .unwrap_or_else(|| TmuxService::session_name(&project_id, &task_id));
                    if self.tmux.session_exists(&session_name) {
                        if let Err(e) = self.tmux.send_prompt(
                            &session_name,
                            task.agent_cli,
                            &self.config.pr_prompt,
                        ) {
                            self.error_message =
                                Some(format!("Failed to send PR instruction: {}", e));
                        }
                    } else {
                        self.error_message =
                            Some("No active tmux session for this task".to_string());
                    }
                }
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

            Action::SendReviewInstruction {
                task_id,
                project_id,
            } => {
                let task = self
                    .tasks_by_project
                    .get(project_id.as_str())
                    .and_then(|tasks| tasks.iter().find(|t| t.id == task_id))
                    .cloned();
                if let Some(task) = task {
                    if task.agent_cli == AgentCli::None {
                        self.error_message =
                            Some("No agent CLI configured for this task".to_string());
                    } else {
                        let session_name = task
                            .tmux_session
                            .clone()
                            .unwrap_or_else(|| TmuxService::session_name(&project_id, &task_id));
                        if self.tmux.session_exists(&session_name) {
                            if let Err(e) = self.tmux.send_prompt(
                                &session_name,
                                task.agent_cli,
                                &self.config.review_prompt,
                            ) {
                                self.error_message =
                                    Some(format!("Failed to send review instruction: {}", e));
                            }
                        } else {
                            self.error_message =
                                Some("No active session for this task".to_string());
                        }
                    }
                }
            }

            Action::OpenCustomPrompt => {
                if let Some(TreeItem::Task { id, project_id, .. }) = self.task_tree.selected_item()
                {
                    self.active_modal = Some(ModalKind::CustomPrompt(CustomPromptModal::new(
                        id.clone(),
                        project_id.clone(),
                        self.config.custom_prompts.clone(),
                    )));
                }
            }

            Action::SendCustomPrompt {
                task_id,
                project_id,
                prompt,
            } => {
                let task = self
                    .tasks_by_project
                    .get(project_id.as_str())
                    .and_then(|tasks| tasks.iter().find(|t| t.id == task_id))
                    .cloned();
                if let Some(task) = task {
                    if task.agent_cli == AgentCli::None {
                        self.error_message =
                            Some("No agent CLI configured for this task".to_string());
                    } else {
                        let session_name = task
                            .tmux_session
                            .clone()
                            .unwrap_or_else(|| TmuxService::session_name(&project_id, &task_id));
                        if self.tmux.session_exists(&session_name) {
                            match self.tmux.send_prompt(&session_name, task.agent_cli, &prompt) {
                                Ok(()) => {
                                    self.active_modal = None;
                                }
                                Err(e) => {
                                    self.error_message =
                                        Some(format!("Failed to send custom prompt: {}", e));
                                }
                            }
                        } else {
                            self.error_message =
                                Some("No active session for this task".to_string());
                        }
                    }
                }
            }

            Action::AddCustomPrompt { prompt } => {
                if self.config.custom_prompts.len() < 5 {
                    let backup = self.config.custom_prompts.clone();
                    self.config.custom_prompts.push(prompt);
                    match self.config.save() {
                        Ok(()) => {
                            if let Some(ModalKind::CustomPrompt(ref mut m)) = self.active_modal {
                                m.update_prompts(self.config.custom_prompts.clone());
                            }
                        }
                        Err(e) => {
                            self.config.custom_prompts = backup;
                            self.error_message = Some(format!("Failed to save config: {}", e));
                        }
                    }
                }
            }

            Action::DeleteCustomPrompt { index } => {
                if index < self.config.custom_prompts.len() {
                    let backup = self.config.custom_prompts.clone();
                    self.config.custom_prompts.remove(index);
                    match self.config.save() {
                        Ok(()) => {
                            if let Some(ModalKind::CustomPrompt(ref mut m)) = self.active_modal {
                                m.update_prompts(self.config.custom_prompts.clone());
                            }
                        }
                        Err(e) => {
                            self.config.custom_prompts = backup;
                            self.error_message = Some(format!("Failed to save config: {}", e));
                        }
                    }
                }
            }

            Action::StartPmSession { project_id } => {
                let pm_enabled = self
                    .projects
                    .iter()
                    .find(|p| p.id == project_id)
                    .map(|p| p.pm_enabled)
                    .unwrap_or(false);
                if !pm_enabled {
                    self.error_message =
                        Some("PM is not enabled for this project. Edit the project (m) to enable it.".to_string());
                } else if let Err(e) = self.handle_pm_trigger(&project_id) {
                    self.error_message = Some(format!("Failed to start PM session: {}", e));
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

                // For PM projects, show file-based preview; for tasks, show tmux pane
                let mut used_file_preview = false;
                if let Some(TreeItem::Project { id, .. }) = self.task_tree.selected_item() {
                    let output_file = self.store.pm_output_file(id);
                    if output_file.exists() {
                        self.preview_panel
                            .update_preview_from_file(&output_file, id);
                        used_file_preview = true;
                    }
                }

                if !used_file_preview {
                    let session_name = self.task_tree.selected_item().and_then(|item| {
                        match item {
                            TreeItem::Task { id, project_id, .. } => {
                                self.tasks_by_project
                                    .get(project_id.as_str())
                                    .and_then(|tasks| tasks.iter().find(|t| t.id == *id))
                                    .and_then(|t| t.tmux_session.as_deref())
                            }
                            TreeItem::Project { .. } => None,
                        }
                    });
                    let session_name_owned = session_name.map(|s| s.to_string());
                    self.preview_panel
                        .update_preview(session_name_owned.as_deref(), &self.tmux);
                }

                // Filesystem change detection: check every 3 seconds for external changes
                // (e.g. tasks created via `ma-task` CLI)
                let fs_check_interval_ticks = (3000 / self.config.tick_rate_ms).max(1);
                let mut data_changed = false;
                if self.tick_count % fs_check_interval_ticks == 0 {
                    let fp = self.store.data_fingerprint();
                    if fp != self.last_data_fingerprint {
                        self.last_data_fingerprint = fp;
                        data_changed = true;
                    }
                }

                // Agent monitor: check every monitor_interval_secs
                // tick_rate_ms=250 → 4 ticks/sec → interval_secs * 4 ticks
                let agent_interval_ticks =
                    (self.config.monitor_interval_secs * 1000 / self.config.tick_rate_ms).max(1);

                if self.tick_count % agent_interval_ticks == 0 {
                    let agent_events = self.agent_monitor.check_all();
                    for event in agent_events {
                        match event {
                            crate::services::agent_monitor::MonitorEvent::StatusChanged {
                                task_id,
                                project_id,
                                status,
                            } => {
                                let result = self.update_task(&project_id, &task_id, |task| {
                                    // Record reopened_at when transitioning from
                                    // Completed so PrMonitor won't auto-complete
                                    // based on already-merged PRs.
                                    if task.status == Status::Completed
                                        && status == Status::InProgress
                                    {
                                        task.reopened_at = Some(Utc::now());
                                    }
                                    task.status = status;
                                    task.updated_at = Utc::now();
                                });
                                // Only clear the marker after the status change
                                // has been persisted, so it can be retried on the
                                // next tick if save fails.
                                if result.is_ok() {
                                    let task_dir = self.store.task_dir(&project_id, &task_id);
                                    if status == Status::InProgress {
                                        let _ = std::fs::remove_file(
                                            task_dir.join(".prompt_submitted"),
                                        );
                                    } else if status == Status::ActionRequired {
                                        let _ =
                                            std::fs::remove_file(task_dir.join(".agent_stopped"));
                                        let _ = std::fs::remove_file(
                                            task_dir.join(".prompt_submitted"),
                                        );
                                    }
                                }
                                data_changed = true;
                            }
                            crate::services::agent_monitor::MonitorEvent::PrLinkDiscovered {
                                task_id,
                                project_id,
                                url,
                            } => {
                                let link = TaskLink {
                                    url,
                                    display_name: None,
                                };
                                let _ = self.update_task(&project_id, &task_id, |task| {
                                    if !task.links.iter().any(|l| l.url == link.url) {
                                        task.links.push(link);
                                        task.updated_at = Utc::now();
                                    }
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
                            // Clear markers so the monitor won't
                            // immediately reopen the task after auto-completing.
                            let task_dir = self.store.task_dir(&project_id, &task_id);
                            let _ = std::fs::remove_file(task_dir.join(".prompt_submitted"));
                            let _ = std::fs::remove_file(task_dir.join(".agent_stopped"));
                            let _ = self.update_task(&project_id, &task_id, |task| {
                                task.status = Status::Completed;
                                task.reopened_at = None;
                                task.updated_at = Utc::now();
                            });
                            data_changed = true;
                        }
                    }
                }

                // PM scheduler: check every 60 seconds
                let pm_interval_ticks = (60_000 / self.config.tick_rate_ms).max(1);
                if self.tick_count.is_multiple_of(pm_interval_ticks) {
                    let now = Utc::now();
                    let pm_events = self.pm_scheduler.check_all(now);
                    for event in pm_events {
                        match event {
                            PmSchedulerEvent::TriggerPm { project_id } => {
                                if let Err(e) = self.handle_pm_trigger(&project_id) {
                                    self.error_message =
                                        Some(format!("PM trigger failed: {}", e));
                                }
                            }
                        }
                    }
                }

                if data_changed {
                    self.reload_data()?;
                    self.rebuild_tree();
                    self.last_data_fingerprint = self.store.data_fingerprint();
                }
            }
        }

        Ok(UpdateResult::Continue)
    }

    fn handle_pm_trigger(&mut self, project_id: &str) -> AppResult<()> {
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_id))?;

        if !project.pm_enabled {
            return Ok(());
        }

        let agent_cli = project
            .pm_agent_cli
            .unwrap_or(crate::domain::task::AgentCli::Claude);
        if agent_cli == AgentCli::None {
            return Ok(());
        }

        let pm_session = TmuxService::pm_session_name(project_id);
        let project_dir = self.store.project_dir(project_id);
        let pm_state_dir = self.store.pm_dir(project_id);
        std::fs::create_dir_all(&pm_state_dir)?;

        let output_file = self.store.pm_output_file(project_id);

        // If agent is currently running interactively (from an attach), kill it first
        if self.tmux.session_exists(&pm_session)
            && self.tmux.is_agent_running_in_session(&pm_session)
        {
            let _ = self.tmux.kill_foreground_process(&pm_session);
        }

        // Remove guard file to cancel any pending Codex prompt thread
        let guard_file = pm_state_dir.join(".pm_prompt_guard");
        let _ = std::fs::remove_file(&guard_file);

        // Kill existing PM session and recreate (fresh start for config reload)
        if self.tmux.session_exists(&pm_session) {
            let _ = self.tmux.kill_session(&pm_session);
        }

        // Write PM config files to project dir (PM inherits project-level skills/settings)
        self.store.write_pm_config_files(&project)?;

        // Create PM session in project dir (not pm subdir)
        self.tmux.create_session(&pm_session, &project_dir)?;

        let trigger_prompt = match agent_cli {
            AgentCli::Claude => "/pm-manager",
            AgentCli::Codex => "$pm-manager",
            AgentCli::Gemini => "pm-managerスキルを使って現況確認を行ってください",
            AgentCli::None => return Ok(()),
        };

        let history_marker = pm_state_dir.join(".has_history");
        // Gemini CLI cannot resume sessions after tmux kill (session data not saved on SIGTERM),
        // so always launch fresh. Claude/Codex handle resume gracefully.
        let can_resume = history_marker.exists() && agent_cli != AgentCli::Gemini;

        // Clear output file before starting
        let _ = std::fs::write(&output_file, "");

        // Launch agent non-interactively with output to file
        self.tmux.launch_agent_non_interactive(
            &pm_session,
            &agent_cli,
            trigger_prompt,
            &output_file,
            can_resume,
        )?;

        if !history_marker.exists() {
            // Mark that a session has been started for future resumes
            let _ = std::fs::write(&history_marker, "");
        }

        // Save PM session name to project
        if let Some(proj) = self.projects.iter().find(|p| p.id == project_id).cloned() {
            let mut updated = proj;
            updated.pm_tmux_session = Some(pm_session.clone());
            updated.updated_at = Utc::now();
            self.store.save_project(&updated)?;
        }

        self.active_sessions.insert(pm_session);
        self.needs_full_redraw = true;
        let _ = self.reload_data();
        self.rebuild_tree();

        Ok(())
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
            let existing = self
                .tasks_by_project
                .get(&project_id)
                .map(|t| t.iter().any(|task| task.id == candidate))
                .unwrap_or(false);
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
            preview_urls: Vec::new(),
            notes,
            initial_instructions: initial_instructions.clone(),
            tmux_session: None,
            created_at: now,
            updated_at: now,
            reopened_at: None,
        };

        self.store.save_task(&task)?;

        // Find project info for background setup
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

        // Spawn background thread for heavy operations (worktree, tmux, config files)
        let store = self.store.clone();
        let tmux = TmuxService::new();
        let (tx, rx) = mpsc::channel();
        let bg_task = task.clone();
        let bg_task_dir = task_dir.clone();

        let pr_prompt = self.config.pr_prompt.clone();
        std::thread::spawn(move || {
            let output = task_setup::run_task_setup(
                TaskSetupInput {
                    task: &bg_task,
                    project: &project,
                    task_dir: &bg_task_dir,
                    pr_prompt,
                },
                &store,
                &tmux,
            );
            let _ = tx.send(TaskSetupResult {
                task_id: bg_task.id.clone(),
                project_id: bg_task.project_id.clone(),
                worktrees: output.worktrees,
                tmux_session: output.tmux_session,
                error: output.error,
            });
        });

        self.task_setup_rx.push(rx);

        Ok(task_id)
    }

    /// Returns Some(session_name) if we should attach to a tmux session,
    /// None otherwise (e.g., toggled expand on project, or no session).
    fn resolve_attach_session(&mut self) -> Option<String> {
        let item = self.task_tree.selected_item().cloned()?;

        match item {
            TreeItem::Project { id, .. } => {
                let project = self.projects.iter().find(|p| p.id == id).cloned();
                let project = match project {
                    Some(p) if p.pm_enabled => p,
                    _ => return None, // No PM — use 'o' to toggle expand instead
                };
                let agent_cli = project.pm_agent_cli.unwrap_or(AgentCli::Claude);
                if agent_cli == AgentCli::None {
                    return None;
                }

                let pm_session = TmuxService::pm_session_name(&id);
                let project_dir = self.store.project_dir(&id);

                if self.tmux.session_exists(&pm_session) {
                    // If a non-interactive agent is running, kill it before interactive attach
                    if self.tmux.is_agent_running_in_session(&pm_session) {
                        let _ = self.tmux.kill_foreground_process(&pm_session);
                    }
                    // Launch agent interactively with resume if not already running
                    if !self.tmux.is_agent_running_in_session(&pm_session) {
                        let pm_state_dir = self.store.pm_dir(&id);
                        let history_marker = pm_state_dir.join(".has_history");
                        let guard_file = pm_state_dir.join(".pm_prompt_guard");
                        let can_resume = history_marker.exists() && agent_cli != AgentCli::Gemini;
                        if can_resume {
                            let _ = self.tmux.launch_agent_resume(
                                &pm_session,
                                &agent_cli,
                                "",
                                Some(&guard_file),
                            );
                        } else {
                            let _ = self.tmux.launch_agent(
                                &pm_session,
                                &agent_cli,
                                None,
                            );
                        }
                    }
                    return Some(pm_session);
                }

                // No PM session — create one and launch agent interactively
                if !TmuxService::is_available() {
                    return None;
                }
                // Write PM config files
                let _ = self.store.write_pm_config_files(&project);
                match self.tmux.create_session(&pm_session, &project_dir) {
                    Ok(()) => {
                        let pm_state_dir = self.store.pm_dir(&id);
                        let _ = std::fs::create_dir_all(&pm_state_dir);
                        let history_marker = pm_state_dir.join(".has_history");
                        let guard_file = pm_state_dir.join(".pm_prompt_guard");
                        let can_resume = history_marker.exists() && agent_cli != AgentCli::Gemini;
                        if can_resume {
                            let _ = self.tmux.launch_agent_resume(
                                &pm_session,
                                &agent_cli,
                                "",
                                Some(&guard_file),
                            );
                        } else {
                            let _ = self.tmux.launch_agent(
                                &pm_session,
                                &agent_cli,
                                None,
                            );
                        }
                        // Save PM session name
                        if let Some(proj) = self.projects.iter().find(|p| p.id == id).cloned() {
                            let mut updated = proj;
                            updated.pm_tmux_session = Some(pm_session.clone());
                            updated.updated_at = Utc::now();
                            let _ = self.store.save_project(&updated);
                        }
                        self.active_sessions.insert(pm_session.clone());
                        self.rebuild_tree();
                        Some(pm_session)
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to create PM session: {}", e));
                        None
                    }
                }
            }
            TreeItem::Task { id, project_id, .. } => {
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

                // Session already exists – just attach
                if self.tmux.session_exists(&session_name) {
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
                            // Always rebuild .initial_prompt from task data so
                            // links added after creation are included.
                            let prompt_file =
                                write_initial_prompt(&task, &task_dir).unwrap_or(None);
                            let _ = self.tmux.launch_agent(
                                &session_name,
                                &task.agent_cli,
                                prompt_file.as_deref(),
                            );
                            // Create .prompt_submitted marker so the monitor can
                            // transition Todo → InProgress.
                            if prompt_file.is_some() {
                                let _ = std::fs::write(
                                    task_dir.join(".prompt_submitted"),
                                    "",
                                );
                            }
                        }
                        // Persist the session name if it wasn't stored yet
                        if task.tmux_session.is_none() {
                            if let Some(tasks) = self.tasks_by_project.get_mut(&project_id) {
                                if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
                                    t.tmux_session = Some(session_name.clone());
                                    if let Err(e) = self.store.save_task(t) {
                                        self.error_message =
                                            Some(format!("Failed to save task: {}", e));
                                    }
                                }
                            }
                        }
                        self.active_sessions.insert(session_name.clone());
                        self.rebuild_tree();
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
        // Remove trust entries from ~/.claude.json to prevent file bloat (best-effort)
        if let Err(e) = self.store.remove_claude_trust(task) {
            eprintln!(
                "warn: failed to clean ~/.claude.json for task {}: {e}",
                task.id
            );
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
        let mut updated: Task =
            serde_json::from_str(&content).with_context(|| format!("parsing {:?}", json_path))?;

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
                    let task_dir = self.store.task_dir(project_id, &task.id);
                    self.preview_panel.update_task_info(
                        task_dir,
                        task.links.clone(),
                        task.notes.clone(),
                        task.initial_instructions.clone(),
                    );
                } else {
                    let task_dir = self.store.task_dir(project_id, id);
                    self.preview_panel.update_task_info(task_dir, Vec::new(), None, None);
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
                            crate::domain::task::Status::ActionRequired => {
                                stats.action_required += 1
                            }
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

                let description = project.and_then(|p| p.description.clone());

                let worktree_copy_files = project
                    .map(|p| p.worktree_copy_files.clone())
                    .unwrap_or_default();

                let project_dir = self.store.project_dir(id);

                let pm_enabled = project.map(|p| p.pm_enabled).unwrap_or(false);
                let pm_agent_cli = project.and_then(|p| p.pm_agent_cli);
                let pm_cron_expression = project.and_then(|p| p.pm_cron_expression.clone());

                self.preview_panel.update_project_info(
                    crate::components::preview_panel::ProjectInfo {
                        name: name.clone(),
                        description,
                        project_dir,
                        repos,
                        worktree_copy_files,
                        task_stats: stats,
                        pm_enabled,
                        pm_agent_cli,
                        pm_cron_expression,
                    },
                );
            }
            None => {
                self.preview_panel.clear_task_info();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let main_chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(area);

        let content_chunks =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
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
            match modal {
                ModalKind::CreateProject(m) => {
                    m.render(frame, centered_rect_with_max(90, 90, 120, 60, area))
                }
                ModalKind::CreateTask(m) => {
                    m.render(frame, centered_rect_with_max(90, 90, 120, 55, area))
                }
                ModalKind::EditItem(m) => {
                    m.render(frame, centered_rect_with_max(90, 90, 120, 60, area))
                }
                ModalKind::SetStatus(m) => m.render(frame, centered_rect(40, 40, area)),
                ModalKind::SetLink(m) => m.render(frame, centered_rect(50, 30, area)),
                ModalKind::SelectLink(m) => m.render(frame, centered_rect(50, 40, area)),
                ModalKind::SelectPreviewUrl(m) => m.render(frame, centered_rect(50, 40, area)),
                ModalKind::Filter(m) => m.render(frame, centered_rect(40, 40, area)),
                ModalKind::Sort(m) => m.render(frame, centered_rect(40, 30, area)),
                ModalKind::ConfirmDelete(m) => m.render(frame, centered_rect(50, 35, area)),
                ModalKind::Settings(m) => m.render(frame, area),
                ModalKind::CustomPrompt(m) => {
                    m.render(frame, centered_rect_with_max(80, 60, 100, 20, area))
                }
            }
        }
    }
}
