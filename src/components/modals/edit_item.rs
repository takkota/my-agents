use super::input::{MultiSelectList, SelectList, TextArea, TextInput};
use super::Modal;
use crate::action::Action;
use crate::domain::task::{AgentCli, Priority};
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::path::PathBuf;

pub enum EditItemModal {
    Project(EditProjectModal),
    Task(EditTaskModal),
}

impl Modal for EditItemModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match self {
            EditItemModal::Project(m) => m.handle_key(key),
            EditItemModal::Task(m) => m.handle_key(key),
        }
    }

    fn handle_paste(&mut self, text: &str) {
        match self {
            EditItemModal::Project(m) => m.handle_paste(text),
            EditItemModal::Task(m) => m.handle_paste(text),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            EditItemModal::Project(m) => m.render(frame, area),
            EditItemModal::Task(m) => m.render(frame, area),
        }
    }
}

// Edit Project
enum ProjectField {
    Name,
    Description,
    CopyFiles,
    DevEnvPrompt,
    Repos,
    PmEnabled,
    PmAgentCli,
    PmCronExpression,
    PmCustomInstructions,
}

pub struct EditProjectModal {
    project_id: String,
    name_input: TextInput,
    description_input: TextInput,
    copy_files_input: TextInput,
    dev_env_prompt_input: TextArea,
    repo_list: MultiSelectList<PathBuf>,
    pm_enabled: bool,
    pm_agent_cli_list: SelectList<AgentCli>,
    pm_cron_input: TextInput,
    pm_custom_instructions_input: TextArea,
    current_field: ProjectField,
}

impl EditProjectModal {
    pub fn new(
        project_id: String,
        current_name: String,
        current_description: Option<String>,
        available_repos: Vec<PathBuf>,
        selected_repos: Vec<PathBuf>,
        current_copy_files: Vec<String>,
        current_dev_env_prompt: Option<String>,
        current_pm_enabled: bool,
        current_pm_agent_cli: Option<AgentCli>,
        current_pm_cron: Option<String>,
        current_pm_custom_instructions: Option<String>,
    ) -> Self {
        let mut name_input = TextInput::new("Project Name").with_value(&current_name);
        name_input.focused = true;

        let description_input = TextInput::new("Description (optional, one-line summary)")
            .with_value(current_description.as_deref().unwrap_or(""));

        let copy_files_str = current_copy_files.join(", ");
        let copy_files_input = TextInput::new("Worktree Copy Files (comma-separated, e.g. .env,.env.local)")
            .with_value(&copy_files_str);

        let dev_env_prompt_input = TextArea::new("Dev Environment Prompt (optional)")
            .with_value(current_dev_env_prompt.as_deref().unwrap_or(""));

        let repo_items: Vec<(String, PathBuf)> = available_repos
            .into_iter()
            .map(|p| {
                let display = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                (display, p)
            })
            .collect();
        let mut repo_list = MultiSelectList::new("Git Repositories", repo_items);
        // Pre-select currently selected repos
        for (_, path, checked) in &mut repo_list.items {
            if selected_repos.contains(path) {
                *checked = true;
            }
        }

        let pm_agent_items: Vec<(String, AgentCli)> = vec![
            ("Claude".to_string(), AgentCli::Claude),
            ("Codex".to_string(), AgentCli::Codex),
            ("Gemini".to_string(), AgentCli::Gemini),
        ];
        let mut pm_agent_cli_list = SelectList::new("PM Agent CLI", pm_agent_items);
        if let Some(cli) = current_pm_agent_cli {
            let idx = match cli {
                AgentCli::Claude => 0,
                AgentCli::Codex => 1,
                AgentCli::Gemini => 2,
                AgentCli::None => 0,
            };
            pm_agent_cli_list.selected = idx;
        }

        let pm_cron_input = TextInput::new("PM Cron Expression (e.g. */30 * * * *)")
            .with_value(current_pm_cron.as_deref().unwrap_or(""));

        let pm_custom_instructions_input = TextArea::new("PM Custom Instructions (optional)")
            .with_value(current_pm_custom_instructions.as_deref().unwrap_or(""));

        Self {
            project_id,
            name_input,
            description_input,
            copy_files_input,
            dev_env_prompt_input,
            repo_list,
            pm_enabled: current_pm_enabled,
            pm_agent_cli_list,
            pm_cron_input,
            pm_custom_instructions_input,
            current_field: ProjectField::Name,
        }
    }

    fn unfocus_all(&mut self) {
        self.name_input.focused = false;
        self.description_input.focused = false;
        self.copy_files_input.focused = false;
        self.dev_env_prompt_input.focused = false;
        self.repo_list.focused = false;
        self.pm_agent_cli_list.focused = false;
        self.pm_cron_input.focused = false;
        self.pm_custom_instructions_input.focused = false;
    }

    fn switch_field(&mut self, forward: bool) {
        self.unfocus_all();
        self.current_field = if forward {
            match self.current_field {
                ProjectField::Name => {
                    self.description_input.focused = true;
                    ProjectField::Description
                }
                ProjectField::Description => {
                    self.copy_files_input.focused = true;
                    ProjectField::CopyFiles
                }
                ProjectField::CopyFiles => {
                    self.dev_env_prompt_input.focused = true;
                    ProjectField::DevEnvPrompt
                }
                ProjectField::DevEnvPrompt => {
                    self.repo_list.focused = true;
                    ProjectField::Repos
                }
                ProjectField::Repos => ProjectField::PmEnabled,
                ProjectField::PmEnabled => {
                    if self.pm_enabled {
                        self.pm_agent_cli_list.focused = true;
                        ProjectField::PmAgentCli
                    } else {
                        self.name_input.focused = true;
                        ProjectField::Name
                    }
                }
                ProjectField::PmAgentCli => {
                    self.pm_cron_input.focused = true;
                    ProjectField::PmCronExpression
                }
                ProjectField::PmCronExpression => {
                    self.pm_custom_instructions_input.focused = true;
                    ProjectField::PmCustomInstructions
                }
                ProjectField::PmCustomInstructions => {
                    self.name_input.focused = true;
                    ProjectField::Name
                }
            }
        } else {
            match self.current_field {
                ProjectField::Name => {
                    if self.pm_enabled {
                        self.pm_custom_instructions_input.focused = true;
                        ProjectField::PmCustomInstructions
                    } else {
                        ProjectField::PmEnabled
                    }
                }
                ProjectField::Description => {
                    self.name_input.focused = true;
                    ProjectField::Name
                }
                ProjectField::CopyFiles => {
                    self.description_input.focused = true;
                    ProjectField::Description
                }
                ProjectField::DevEnvPrompt => {
                    self.copy_files_input.focused = true;
                    ProjectField::CopyFiles
                }
                ProjectField::Repos => {
                    self.dev_env_prompt_input.focused = true;
                    ProjectField::DevEnvPrompt
                }
                ProjectField::PmEnabled => {
                    self.repo_list.focused = true;
                    ProjectField::Repos
                }
                ProjectField::PmAgentCli => ProjectField::PmEnabled,
                ProjectField::PmCronExpression => {
                    self.pm_agent_cli_list.focused = true;
                    ProjectField::PmAgentCli
                }
                ProjectField::PmCustomInstructions => {
                    self.pm_cron_input.focused = true;
                    ProjectField::PmCronExpression
                }
            }
        };
    }
}

impl Modal for EditProjectModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // In TextArea fields, pass Enter (newline) and Up/Down to TextArea
        if matches!(self.current_field, ProjectField::DevEnvPrompt | ProjectField::PmCustomInstructions) {
            if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
                match self.current_field {
                    ProjectField::DevEnvPrompt => self.dev_env_prompt_input.insert_newline(),
                    ProjectField::PmCustomInstructions => self.pm_custom_instructions_input.insert_newline(),
                    _ => {}
                }
                return Ok(None);
            }
            if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                match self.current_field {
                    ProjectField::DevEnvPrompt => { self.dev_env_prompt_input.handle_key(key); },
                    ProjectField::PmCustomInstructions => { self.pm_custom_instructions_input.handle_key(key); },
                    _ => {}
                }
                return Ok(None);
            }
        }
        // Ctrl+Enter submits the form
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.name_input.value.is_empty() {
                return Ok(None);
            }
            let repos: Vec<(String, PathBuf)> = self
                .repo_list
                .selected_values()
                .into_iter()
                .map(|p| {
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    (name, p.clone())
                })
                .collect();

            let description = if self.description_input.value.is_empty() {
                None
            } else {
                Some(self.description_input.value.clone())
            };

            let dev_environment_prompt = if self.dev_env_prompt_input.value.is_empty() {
                None
            } else {
                Some(self.dev_env_prompt_input.value.clone())
            };

            let pm_agent_cli = if self.pm_enabled {
                self.pm_agent_cli_list.selected_value().copied()
            } else {
                None
            };

            let pm_cron_expression = if self.pm_enabled && !self.pm_cron_input.value.is_empty() {
                Some(self.pm_cron_input.value.clone())
            } else {
                None
            };

            let pm_custom_instructions = if self.pm_enabled && !self.pm_custom_instructions_input.value.is_empty() {
                Some(self.pm_custom_instructions_input.value.clone())
            } else {
                None
            };

            return Ok(Some(Action::UpdateProject {
                project_id: self.project_id.clone(),
                name: self.name_input.value.clone(),
                description,
                repos,
                worktree_copy_files: super::parse_comma_separated(&self.copy_files_input.value),
                dev_environment_prompt,
                pm_enabled: self.pm_enabled,
                pm_agent_cli,
                pm_custom_instructions,
                pm_cron_expression,
            }));
        }
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Tab => {
                self.switch_field(true);
                Ok(None)
            }
            KeyCode::BackTab => {
                self.switch_field(false);
                Ok(None)
            }
            _ => {
                match self.current_field {
                    ProjectField::Name => { self.name_input.handle_key(key); },
                    ProjectField::Description => { self.description_input.handle_key(key); },
                    ProjectField::CopyFiles => { self.copy_files_input.handle_key(key); },
                    ProjectField::DevEnvPrompt => { self.dev_env_prompt_input.handle_key(key); },
                    ProjectField::Repos => match key.code {
                        KeyCode::Up => self.repo_list.move_up(),
                        KeyCode::Down => self.repo_list.move_down(),
                        KeyCode::Char(' ') => self.repo_list.toggle(),
                        KeyCode::Backspace => {
                            self.repo_list.filter_text.pop();
                            self.repo_list.cursor = 0;
                        }
                        KeyCode::Char(c) => {
                            self.repo_list.filter_text.push(c);
                            self.repo_list.cursor = 0;
                        }
                        _ => {}
                    },
                    ProjectField::PmEnabled => match key.code {
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            self.pm_enabled = !self.pm_enabled;
                        }
                        _ => {}
                    },
                    ProjectField::PmAgentCli => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.pm_agent_cli_list.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.pm_agent_cli_list.move_down(),
                        _ => {}
                    },
                    ProjectField::PmCronExpression => { self.pm_cron_input.handle_key(key); },
                    ProjectField::PmCustomInstructions => { self.pm_custom_instructions_input.handle_key(key); },
                }
                Ok(None)
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Edit Project ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let mut constraints = vec![
            Constraint::Length(3), // Name
            Constraint::Length(3), // Description
            Constraint::Length(3), // CopyFiles
            Constraint::Length(5), // DevEnvPrompt
            Constraint::Length(5), // Repos
            Constraint::Length(1), // PM toggle
        ];

        if self.pm_enabled {
            constraints.push(Constraint::Length(5)); // PM Agent CLI
            constraints.push(Constraint::Length(3)); // PM Cron
            constraints.push(Constraint::Min(3));   // PM Custom Instructions
        } else {
            constraints.push(Constraint::Min(0));
        }

        let chunks = Layout::vertical(constraints).split(inner);

        self.name_input.render(frame, chunks[0]);
        self.description_input.render(frame, chunks[1]);
        self.copy_files_input.render(frame, chunks[2]);
        self.dev_env_prompt_input.render(frame, chunks[3]);
        self.repo_list.render(frame, chunks[4]);

        // PM toggle
        let pm_focused = matches!(self.current_field, ProjectField::PmEnabled);
        let toggle_style = if pm_focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let toggle_text = if self.pm_enabled { "[x] PM Enabled" } else { "[ ] PM Enabled" };
        let toggle_line = Line::from(vec![
            Span::styled(toggle_text, toggle_style),
            Span::styled("  (Space to toggle)", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(toggle_line), chunks[5]);

        if self.pm_enabled {
            self.pm_agent_cli_list.render(frame, chunks[6]);
            self.pm_cron_input.render(frame, chunks[7]);
            self.pm_custom_instructions_input.render(frame, chunks[8]);
        }
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            ProjectField::Name => self.name_input.insert_paste(text),
            ProjectField::Description => self.description_input.insert_paste(text),
            ProjectField::CopyFiles => self.copy_files_input.insert_paste(text),
            ProjectField::DevEnvPrompt => self.dev_env_prompt_input.insert_paste(text),
            ProjectField::PmCronExpression => self.pm_cron_input.insert_paste(text),
            ProjectField::PmCustomInstructions => self.pm_custom_instructions_input.insert_paste(text),
            ProjectField::Repos | ProjectField::PmEnabled | ProjectField::PmAgentCli => {}
        }
    }
}

// Edit Task (name + priority + notes)
enum TaskField {
    Name,
    Notes,
    Priority,
}

pub struct EditTaskModal {
    task_id: String,
    project_id: String,
    name_input: TextInput,
    notes_input: TextArea,
    priority_list: SelectList<Priority>,
    current_field: TaskField,
}

impl EditTaskModal {
    pub fn new(
        task_id: String,
        project_id: String,
        current_name: String,
        current_priority: Priority,
        current_notes: Option<String>,
    ) -> Self {
        let mut name_input = TextInput::new("Task Name").with_value(&current_name);
        name_input.focused = true;
        let notes_input =
            TextArea::new("Notes").with_value(current_notes.as_deref().unwrap_or(""));

        let priority_items: Vec<(String, Priority)> = Priority::all()
            .iter()
            .map(|p| (p.to_string(), *p))
            .collect();
        let mut priority_list = SelectList::new("Priority", priority_items);
        priority_list.selected = Priority::all()
            .iter()
            .position(|p| *p == current_priority)
            .unwrap_or(2);

        Self {
            task_id,
            project_id,
            name_input,
            notes_input,
            priority_list,
            current_field: TaskField::Name,
        }
    }

    fn switch_field(&mut self, forward: bool) {
        self.name_input.focused = false;
        self.notes_input.focused = false;
        self.priority_list.focused = false;

        self.current_field = if forward {
            match self.current_field {
                TaskField::Name => TaskField::Notes,
                TaskField::Notes => TaskField::Priority,
                TaskField::Priority => TaskField::Name,
            }
        } else {
            match self.current_field {
                TaskField::Name => TaskField::Priority,
                TaskField::Notes => TaskField::Name,
                TaskField::Priority => TaskField::Notes,
            }
        };

        match self.current_field {
            TaskField::Name => self.name_input.focused = true,
            TaskField::Notes => self.notes_input.focused = true,
            TaskField::Priority => self.priority_list.focused = true,
        }
    }
}

impl Modal for EditTaskModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // In Notes field, pass Enter (newline) and Up/Down (line nav) to TextArea
        if matches!(self.current_field, TaskField::Notes) {
            if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
                self.notes_input.insert_newline();
                return Ok(None);
            }
            if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.notes_input.handle_key(key);
                return Ok(None);
            }
        }
        // Ctrl+Enter submits the form
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.name_input.value.is_empty() {
                return Ok(None);
            }
            let priority = self
                .priority_list
                .selected_value()
                .copied()
                .unwrap_or(Priority::P3);
            let notes = if self.notes_input.value.is_empty() {
                None
            } else {
                Some(self.notes_input.value.clone())
            };
            return Ok(Some(Action::UpdateTask {
                task_id: self.task_id.clone(),
                project_id: self.project_id.clone(),
                name: self.name_input.value.clone(),
                priority,
                notes,
            }));
        }
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Tab => {
                self.switch_field(true);
                Ok(None)
            }
            KeyCode::BackTab => {
                self.switch_field(false);
                Ok(None)
            }
            _ => {
                match self.current_field {
                    TaskField::Name => { self.name_input.handle_key(key); },
                    TaskField::Notes => { self.notes_input.handle_key(key); },
                    TaskField::Priority => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.priority_list.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.priority_list.move_down(),
                        _ => {}
                    },
                }
                Ok(None)
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Edit Task ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(7),
        ])
        .split(inner);

        self.name_input.render(frame, chunks[0]);
        self.notes_input.render(frame, chunks[1]);
        self.priority_list.render(frame, chunks[2]);
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            TaskField::Name => self.name_input.insert_paste(text),
            TaskField::Notes => self.notes_input.insert_paste(text),
            TaskField::Priority => {}
        }
    }
}
