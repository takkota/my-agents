use super::input::{MultiSelectList, TextInput};
use super::Modal;
use crate::action::Action;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;
use std::path::PathBuf;

enum Field {
    Name,
    Description,
    CopyFiles,
    Repos,
}

pub struct CreateProjectModal {
    name_input: TextInput,
    description_input: TextInput,
    copy_files_input: TextInput,
    repo_list: MultiSelectList<PathBuf>,
    current_field: Field,
}

impl CreateProjectModal {
    pub fn new(available_repos: Vec<PathBuf>) -> Self {
        let repo_items: Vec<(String, PathBuf)> = available_repos
            .into_iter()
            .map(|p| {
                let display = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                (display, p)
            })
            .collect();

        let mut name_input = TextInput::new("Project Name (alphanumeric, hyphens)");
        name_input.focused = true;
        let description_input = TextInput::new("Description (optional, one-line summary)");
        let copy_files_input = TextInput::new("Worktree Copy Files (comma-separated, e.g. .env,.env.local)");

        Self {
            name_input,
            description_input,
            copy_files_input,
            repo_list: MultiSelectList::new("Git Repositories (Space to toggle)", repo_items),
            current_field: Field::Name,
        }
    }

    fn switch_field(&mut self, forward: bool) {
        self.name_input.focused = false;
        self.description_input.focused = false;
        self.copy_files_input.focused = false;
        self.repo_list.focused = false;
        self.current_field = if forward {
            match self.current_field {
                Field::Name => {
                    self.description_input.focused = true;
                    Field::Description
                }
                Field::Description => {
                    self.copy_files_input.focused = true;
                    Field::CopyFiles
                }
                Field::CopyFiles => {
                    self.repo_list.focused = true;
                    Field::Repos
                }
                Field::Repos => {
                    self.name_input.focused = true;
                    Field::Name
                }
            }
        } else {
            match self.current_field {
                Field::Name => {
                    self.repo_list.focused = true;
                    Field::Repos
                }
                Field::Description => {
                    self.name_input.focused = true;
                    Field::Name
                }
                Field::CopyFiles => {
                    self.description_input.focused = true;
                    Field::Description
                }
                Field::Repos => {
                    self.copy_files_input.focused = true;
                    Field::CopyFiles
                }
            }
        };
    }

    fn validate_name(name: &str) -> bool {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    }
}

impl Modal for CreateProjectModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // Ctrl+Enter submits the form
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            if !Self::validate_name(&self.name_input.value) {
                return Ok(None);
            }
            let repos: Vec<(String, PathBuf)> = self
                .repo_list
                .selected_values()
                .into_iter()
                .map(|p| {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    (name, p.clone())
                })
                .collect();

            let description = if self.description_input.value.is_empty() {
                None
            } else {
                Some(self.description_input.value.clone())
            };

            return Ok(Some(Action::CreateProject {
                name: self.name_input.value.clone(),
                description,
                repos,
                worktree_copy_files: super::parse_comma_separated(&self.copy_files_input.value),
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
                    Field::Name => { self.name_input.handle_key(key); },
                    Field::Description => { self.description_input.handle_key(key); },
                    Field::CopyFiles => { self.copy_files_input.handle_key(key); },
                    Field::Repos => match key.code {
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
            .title(" New Project ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
        ])
        .split(inner);

        self.name_input.render(frame, chunks[0]);
        self.description_input.render(frame, chunks[1]);
        self.copy_files_input.render(frame, chunks[2]);
        self.repo_list.render(frame, chunks[3]);
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            Field::Name => self.name_input.insert_paste(text),
            Field::Description => self.description_input.insert_paste(text),
            Field::CopyFiles => self.copy_files_input.insert_paste(text),
            Field::Repos => {}
        }
    }
}
