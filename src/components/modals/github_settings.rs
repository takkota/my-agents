use super::input::{MultiSelectList, SelectList, TextArea, TextInput};
use super::Modal;
use crate::action::Action;
use crate::domain::task::AgentCli;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

enum Field {
    Enabled,
    Repos,
    Labels,
    AgentCli,
    InitialPrompt,
}

pub struct GithubSettingsModal {
    project_id: String,
    enabled: bool,
    repo_list: MultiSelectList<String>,
    labels_input: TextInput,
    agent_cli_list: SelectList<AgentCli>,
    initial_prompt_input: TextArea,
    current_field: Field,
    validation_error: Option<String>,
}

impl GithubSettingsModal {
    /// `project_repos`: the project's repos as `(display_name, repo_name)`
    /// (repo_name = the `RepoRef.name` that `issue_monitor_repos` stores).
    /// `current_*`: the current issue-monitor config values.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: String,
        project_repos: Vec<(String, String)>,
        current_enabled: bool,
        current_repos: Vec<String>,
        current_labels: Vec<String>,
        current_agent_cli: Option<AgentCli>,
        default_agent_cli: AgentCli,
        current_initial_prompt: Option<String>,
    ) -> Self {
        let mut repo_list = MultiSelectList::new("Repositories to Watch", project_repos);
        // Pre-select currently monitored repos.
        for (_, name, checked) in &mut repo_list.items {
            if current_repos.contains(name) {
                *checked = true;
            }
        }

        let labels_str = current_labels.join(", ");
        let labels_input = TextInput::new("Labels (comma-separated, AND-match; empty = any)")
            .with_value(&labels_str);

        let agent_items: Vec<(String, AgentCli)> = vec![
            ("Claude".to_string(), AgentCli::Claude),
            ("Codex".to_string(), AgentCli::Codex),
            ("Gemini".to_string(), AgentCli::Gemini),
            ("Cursor".to_string(), AgentCli::Cursor),
            ("Devin".to_string(), AgentCli::Devin),
        ];
        let mut agent_cli_list = SelectList::new("Agent CLI (for auto tasks)", agent_items);
        let start_cli = current_agent_cli.unwrap_or(default_agent_cli);
        let idx = match start_cli {
            AgentCli::Claude => 0,
            AgentCli::Codex => 1,
            AgentCli::Gemini => 2,
            AgentCli::Cursor => 3,
            AgentCli::Devin => 4,
            AgentCli::None => 0,
        };
        agent_cli_list.selected = idx;

        let initial_prompt_input =
            TextArea::new("Initial Prompt (issue URL is appended automatically)")
                .with_value(current_initial_prompt.as_deref().unwrap_or(""));

        Self {
            project_id,
            enabled: current_enabled,
            repo_list,
            labels_input,
            agent_cli_list,
            initial_prompt_input,
            current_field: Field::Enabled,
            validation_error: None,
        }
    }

    fn unfocus_all(&mut self) {
        self.repo_list.focused = false;
        self.labels_input.focused = false;
        self.agent_cli_list.focused = false;
        self.initial_prompt_input.focused = false;
    }

    fn switch_field(&mut self, forward: bool) {
        self.unfocus_all();
        self.current_field = if forward {
            match self.current_field {
                Field::Enabled => {
                    if self.enabled {
                        self.repo_list.focused = true;
                        Field::Repos
                    } else {
                        Field::Enabled
                    }
                }
                Field::Repos => {
                    self.labels_input.focused = true;
                    Field::Labels
                }
                Field::Labels => {
                    self.agent_cli_list.focused = true;
                    Field::AgentCli
                }
                Field::AgentCli => {
                    self.initial_prompt_input.focused = true;
                    Field::InitialPrompt
                }
                Field::InitialPrompt => Field::Enabled,
            }
        } else {
            match self.current_field {
                Field::Enabled => {
                    if self.enabled {
                        self.initial_prompt_input.focused = true;
                        Field::InitialPrompt
                    } else {
                        Field::Enabled
                    }
                }
                Field::Repos => Field::Enabled,
                Field::Labels => {
                    self.repo_list.focused = true;
                    Field::Repos
                }
                Field::AgentCli => {
                    self.labels_input.focused = true;
                    Field::Labels
                }
                Field::InitialPrompt => {
                    self.agent_cli_list.focused = true;
                    Field::AgentCli
                }
            }
        };
    }
}

impl Modal for GithubSettingsModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // In the TextArea, Enter inserts a newline; Up/Down navigate lines.
        if matches!(self.current_field, Field::InitialPrompt) {
            if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
                self.initial_prompt_input.insert_newline();
                return Ok(None);
            }
            if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.initial_prompt_input.handle_key(key);
                return Ok(None);
            }
        }

        // Ctrl+Enter submits the form
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.validation_error = None;
            if self.enabled {
                if self.repo_list.items.is_empty() {
                    self.validation_error = Some(
                        "No repositories available; add repos to the project first.".to_string(),
                    );
                    return Ok(None);
                }
                if self.repo_list.selected_values().is_empty() {
                    self.validation_error =
                        Some("Select at least one repository to watch.".to_string());
                    return Ok(None);
                }
            }

            let repos: Vec<String> = if self.enabled {
                self.repo_list
                    .selected_values()
                    .iter()
                    .map(|s| (*s).clone())
                    .collect()
            } else {
                Vec::new()
            };
            let labels = if self.enabled {
                super::parse_comma_separated(&self.labels_input.value)
            } else {
                Vec::new()
            };
            let agent_cli = if self.enabled {
                self.agent_cli_list.selected_value().copied()
            } else {
                None
            };
            let initial_prompt = if self.enabled && !self.initial_prompt_input.value.is_empty() {
                Some(self.initial_prompt_input.value.clone())
            } else {
                None
            };

            return Ok(Some(Action::SaveGithubSettings {
                project_id: self.project_id.clone(),
                enabled: self.enabled,
                repos,
                labels,
                agent_cli,
                initial_prompt,
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
                    Field::Enabled => {
                        if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                            self.enabled = !self.enabled;
                        }
                    }
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
                    Field::Labels => {
                        self.labels_input.handle_key(key);
                    }
                    Field::AgentCli => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.agent_cli_list.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.agent_cli_list.move_down(),
                        _ => {}
                    },
                    Field::InitialPrompt => {
                        self.initial_prompt_input.handle_key(key);
                    }
                }
                Ok(None)
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let title = if let Some(err) = &self.validation_error {
            format!(" GitHub Issue Monitor - {} ", err)
        } else {
            " GitHub Issue Monitor ".to_string()
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

        let constraints = if self.enabled {
            vec![
                Constraint::Length(1), // Enabled toggle
                Constraint::Min(8),    // Repos
                Constraint::Length(3), // Labels
                Constraint::Length(7), // Agent CLI (5 items + 2 border)
                Constraint::Min(5),    // Initial prompt
            ]
        } else {
            vec![Constraint::Length(1)]
        };
        let chunks = Layout::vertical(constraints).split(inner);

        // Enabled toggle
        let focused = matches!(self.current_field, Field::Enabled);
        let toggle_style = if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let toggle_text = if self.enabled {
            "[x] Monitor GitHub Issues"
        } else {
            "[ ] Monitor GitHub Issues"
        };
        let toggle_line = Line::from(vec![
            Span::styled(toggle_text, toggle_style),
            Span::styled("  (Space to toggle)", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(toggle_line), chunks[0]);

        if self.enabled {
            self.repo_list.render(frame, chunks[1]);
            self.labels_input.render(frame, chunks[2]);
            self.agent_cli_list.render(frame, chunks[3]);
            self.initial_prompt_input.render(frame, chunks[4]);
        }
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            Field::Labels => self.labels_input.insert_paste(text),
            Field::InitialPrompt => self.initial_prompt_input.insert_paste(text),
            _ => {}
        }
    }
}
