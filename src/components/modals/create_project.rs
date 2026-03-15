use super::input::{MultiSelectList, SelectList, TextArea, TextInput};
use super::Modal;
use crate::action::Action;
use crate::domain::task::AgentCli;
use crate::error::AppResult;
use crate::services::pm_scheduler::validate_cron;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::path::PathBuf;

enum Field {
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

pub struct CreateProjectModal {
    name_input: TextInput,
    description_input: TextInput,
    copy_files_input: TextInput,
    dev_env_prompt_input: TextArea,
    repo_list: MultiSelectList<PathBuf>,
    pm_enabled: bool,
    pm_agent_cli_list: SelectList<AgentCli>,
    pm_cron_input: TextInput,
    pm_custom_instructions_input: TextArea,
    current_field: Field,
    validation_error: Option<String>,
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
        let dev_env_prompt_input = TextArea::new("Dev Environment Prompt (optional)");

        let pm_agent_items: Vec<(String, AgentCli)> = vec![
            ("Claude".to_string(), AgentCli::Claude),
            ("Codex".to_string(), AgentCli::Codex),
            ("Gemini".to_string(), AgentCli::Gemini),
        ];
        let pm_agent_cli_list = SelectList::new("PM Agent CLI", pm_agent_items);
        let pm_cron_input = TextInput::new("PM Cron Expression (e.g. */30 * * * *)");
        let pm_custom_instructions_input = TextArea::new("PM Custom Instructions (optional)");

        Self {
            name_input,
            description_input,
            copy_files_input,
            dev_env_prompt_input,
            repo_list: MultiSelectList::new("Git Repositories (Space to toggle)", repo_items),
            pm_enabled: false,
            pm_agent_cli_list,
            pm_cron_input,
            pm_custom_instructions_input,
            current_field: Field::Name,
            validation_error: None,
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
                Field::Name => {
                    self.description_input.focused = true;
                    Field::Description
                }
                Field::Description => {
                    self.copy_files_input.focused = true;
                    Field::CopyFiles
                }
                Field::CopyFiles => {
                    self.dev_env_prompt_input.focused = true;
                    Field::DevEnvPrompt
                }
                Field::DevEnvPrompt => {
                    self.repo_list.focused = true;
                    Field::Repos
                }
                Field::Repos => Field::PmEnabled,
                Field::PmEnabled => {
                    if self.pm_enabled {
                        self.pm_agent_cli_list.focused = true;
                        Field::PmAgentCli
                    } else {
                        self.name_input.focused = true;
                        Field::Name
                    }
                }
                Field::PmAgentCli => {
                    self.pm_cron_input.focused = true;
                    Field::PmCronExpression
                }
                Field::PmCronExpression => {
                    self.pm_custom_instructions_input.focused = true;
                    Field::PmCustomInstructions
                }
                Field::PmCustomInstructions => {
                    self.name_input.focused = true;
                    Field::Name
                }
            }
        } else {
            match self.current_field {
                Field::Name => {
                    if self.pm_enabled {
                        self.pm_custom_instructions_input.focused = true;
                        Field::PmCustomInstructions
                    } else {
                        Field::PmEnabled
                    }
                }
                Field::Description => {
                    self.name_input.focused = true;
                    Field::Name
                }
                Field::CopyFiles => {
                    self.description_input.focused = true;
                    Field::Description
                }
                Field::DevEnvPrompt => {
                    self.copy_files_input.focused = true;
                    Field::CopyFiles
                }
                Field::Repos => {
                    self.dev_env_prompt_input.focused = true;
                    Field::DevEnvPrompt
                }
                Field::PmEnabled => {
                    self.repo_list.focused = true;
                    Field::Repos
                }
                Field::PmAgentCli => Field::PmEnabled,
                Field::PmCronExpression => {
                    self.pm_agent_cli_list.focused = true;
                    Field::PmAgentCli
                }
                Field::PmCustomInstructions => {
                    self.pm_cron_input.focused = true;
                    Field::PmCronExpression
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
        // In TextArea fields, pass Enter (newline) and Up/Down to TextArea
        if matches!(self.current_field, Field::DevEnvPrompt | Field::PmCustomInstructions) {
            if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
                match self.current_field {
                    Field::DevEnvPrompt => self.dev_env_prompt_input.insert_newline(),
                    Field::PmCustomInstructions => self.pm_custom_instructions_input.insert_newline(),
                    _ => {}
                }
                return Ok(None);
            }
            if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                match self.current_field {
                    Field::DevEnvPrompt => { self.dev_env_prompt_input.handle_key(key); },
                    Field::PmCustomInstructions => { self.pm_custom_instructions_input.handle_key(key); },
                    _ => {}
                }
                return Ok(None);
            }
        }
        // Ctrl+Enter submits the form
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.validation_error = None;
            if !Self::validate_name(&self.name_input.value) {
                self.validation_error = Some("Project name must be alphanumeric with hyphens/underscores".to_string());
                return Ok(None);
            }

            // Validate cron expression if PM is enabled
            if self.pm_enabled && !self.pm_cron_input.value.is_empty() {
                if let Err(e) = validate_cron(&self.pm_cron_input.value) {
                    self.validation_error = Some(format!("Invalid cron: {}", e));
                    return Ok(None);
                }
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

            return Ok(Some(Action::CreateProject {
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
                    Field::Name => { self.name_input.handle_key(key); },
                    Field::Description => { self.description_input.handle_key(key); },
                    Field::CopyFiles => { self.copy_files_input.handle_key(key); },
                    Field::DevEnvPrompt => { self.dev_env_prompt_input.handle_key(key); },
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
                    Field::PmEnabled => match key.code {
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            self.pm_enabled = !self.pm_enabled;
                        }
                        _ => {}
                    },
                    Field::PmAgentCli => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.pm_agent_cli_list.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.pm_agent_cli_list.move_down(),
                        _ => {}
                    },
                    Field::PmCronExpression => { self.pm_cron_input.handle_key(key); },
                    Field::PmCustomInstructions => { self.pm_custom_instructions_input.handle_key(key); },
                }
                Ok(None)
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let title = if let Some(err) = &self.validation_error {
            format!(" New Project - {} ", err)
        } else {
            " New Project ".to_string()
        };
        let border_color = if self.validation_error.is_some() {
            Color::Red
        } else {
            Color::Cyan
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color));
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
            Constraint::Length(5), // Repos (shrunk)
            Constraint::Length(1), // PM toggle
        ];

        if self.pm_enabled {
            constraints.push(Constraint::Length(5)); // PM Agent CLI
            constraints.push(Constraint::Length(3)); // PM Cron
            constraints.push(Constraint::Min(3));   // PM Custom Instructions
        } else {
            constraints.push(Constraint::Min(0)); // spacer
        }

        let chunks = Layout::vertical(constraints).split(inner);

        self.name_input.render(frame, chunks[0]);
        self.description_input.render(frame, chunks[1]);
        self.copy_files_input.render(frame, chunks[2]);
        self.dev_env_prompt_input.render(frame, chunks[3]);
        self.repo_list.render(frame, chunks[4]);

        // PM toggle
        let pm_focused = matches!(self.current_field, Field::PmEnabled);
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
            Field::Name => self.name_input.insert_paste(text),
            Field::Description => self.description_input.insert_paste(text),
            Field::CopyFiles => self.copy_files_input.insert_paste(text),
            Field::DevEnvPrompt => self.dev_env_prompt_input.insert_paste(text),
            Field::PmCronExpression => self.pm_cron_input.insert_paste(text),
            Field::PmCustomInstructions => self.pm_custom_instructions_input.insert_paste(text),
            Field::Repos | Field::PmEnabled | Field::PmAgentCli => {}
        }
    }
}
