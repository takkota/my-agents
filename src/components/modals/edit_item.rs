use super::input::{MultiSelectList, SelectList, TextInput};
use super::Modal;
use crate::action::Action;
use crate::domain::task::Priority;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
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
    CopyFiles,
    Repos,
}

pub struct EditProjectModal {
    project_id: String,
    name_input: TextInput,
    copy_files_input: TextInput,
    repo_list: MultiSelectList<PathBuf>,
    current_field: ProjectField,
}

impl EditProjectModal {
    pub fn new(
        project_id: String,
        current_name: String,
        available_repos: Vec<PathBuf>,
        selected_repos: Vec<PathBuf>,
        current_copy_files: Vec<String>,
    ) -> Self {
        let mut name_input = TextInput::new("Project Name").with_value(&current_name);
        name_input.focused = true;

        let copy_files_str = current_copy_files.join(", ");
        let copy_files_input = TextInput::new("Worktree Copy Files (comma-separated, e.g. .env,.env.local)")
            .with_value(&copy_files_str);

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

        Self {
            project_id,
            name_input,
            copy_files_input,
            repo_list,
            current_field: ProjectField::Name,
        }
    }

    fn switch_field(&mut self, forward: bool) {
        self.name_input.focused = false;
        self.copy_files_input.focused = false;
        self.repo_list.focused = false;
        self.current_field = if forward {
            match self.current_field {
                ProjectField::Name => {
                    self.copy_files_input.focused = true;
                    ProjectField::CopyFiles
                }
                ProjectField::CopyFiles => {
                    self.repo_list.focused = true;
                    ProjectField::Repos
                }
                ProjectField::Repos => {
                    self.name_input.focused = true;
                    ProjectField::Name
                }
            }
        } else {
            match self.current_field {
                ProjectField::Name => {
                    self.repo_list.focused = true;
                    ProjectField::Repos
                }
                ProjectField::CopyFiles => {
                    self.name_input.focused = true;
                    ProjectField::Name
                }
                ProjectField::Repos => {
                    self.copy_files_input.focused = true;
                    ProjectField::CopyFiles
                }
            }
        };
    }
}

impl Modal for EditProjectModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
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
            KeyCode::Enter => {
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

                Ok(Some(Action::UpdateProject {
                    project_id: self.project_id.clone(),
                    name: self.name_input.value.clone(),
                    repos,
                    worktree_copy_files: super::parse_comma_separated(&self.copy_files_input.value),
                }))
            }
            _ => {
                match self.current_field {
                    ProjectField::Name => { self.name_input.handle_key(key); },
                    ProjectField::CopyFiles => { self.copy_files_input.handle_key(key); },
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
        let chunks =
            Layout::vertical([Constraint::Length(3), Constraint::Length(3), Constraint::Min(5)]).split(inner);

        self.name_input.render(frame, chunks[0]);
        self.copy_files_input.render(frame, chunks[1]);
        self.repo_list.render(frame, chunks[2]);
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            ProjectField::Name => self.name_input.insert_paste(text),
            ProjectField::CopyFiles => self.copy_files_input.insert_paste(text),
            ProjectField::Repos => {}
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
    notes_input: TextInput,
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
            TextInput::new("Notes").with_value(current_notes.as_deref().unwrap_or(""));

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
            KeyCode::Enter => {
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
                Ok(Some(Action::UpdateTask {
                    task_id: self.task_id.clone(),
                    project_id: self.project_id.clone(),
                    name: self.name_input.value.clone(),
                    priority,
                    notes,
                }))
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
            Constraint::Length(3),
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
