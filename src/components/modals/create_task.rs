use super::input::{SelectList, TextArea, TextInput};
use super::Modal;
use crate::action::Action;
use crate::domain::task::{AgentCli, Priority, TaskLink};
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

enum Field {
    Name,
    Notes,
    LinkUrl,
    Priority,
    AgentCli,
}

const MAX_LINK_LINES: usize = 5;

pub struct CreateTaskModal {
    project_id: String,
    name_input: TextInput,
    notes_input: TextArea,
    link_url_input: TextArea,
    priority_list: SelectList<Priority>,
    agent_list: SelectList<AgentCli>,
    current_field: Field,
}

impl CreateTaskModal {
    pub fn new(project_id: String, default_agent: AgentCli) -> Self {
        let mut name_input = TextInput::new("Task Name");
        name_input.focused = true;

        let notes_input = TextArea::new("Notes");
        let link_url_input = TextArea::new("Link URLs (one per line, max 5)");

        let priority_items: Vec<(String, Priority)> = Priority::all()
            .iter()
            .map(|p| (p.to_string(), *p))
            .collect();
        let mut priority_list = SelectList::new("Priority", priority_items);
        priority_list.selected = 2; // P3 default

        let agent_items: Vec<(String, AgentCli)> = AgentCli::all()
            .iter()
            .map(|a| (a.to_string(), *a))
            .collect();
        let mut agent_list = SelectList::new("Agent CLI", agent_items);
        agent_list.selected = AgentCli::all()
            .iter()
            .position(|a| *a == default_agent)
            .unwrap_or(0);

        Self {
            project_id,
            name_input,
            notes_input,
            link_url_input,
            priority_list,
            agent_list,
            current_field: Field::Name,
        }
    }

    fn switch_field(&mut self, forward: bool) {
        self.name_input.focused = false;
        self.notes_input.focused = false;
        self.link_url_input.focused = false;
        self.priority_list.focused = false;
        self.agent_list.focused = false;

        self.current_field = if forward {
            match self.current_field {
                Field::Name => Field::Notes,
                Field::Notes => Field::LinkUrl,
                Field::LinkUrl => Field::Priority,
                Field::Priority => Field::AgentCli,
                Field::AgentCli => Field::Name,
            }
        } else {
            match self.current_field {
                Field::Name => Field::AgentCli,
                Field::Notes => Field::Name,
                Field::LinkUrl => Field::Notes,
                Field::Priority => Field::LinkUrl,
                Field::AgentCli => Field::Priority,
            }
        };

        match self.current_field {
            Field::Name => self.name_input.focused = true,
            Field::Notes => self.notes_input.focused = true,
            Field::LinkUrl => self.link_url_input.focused = true,
            Field::Priority => self.priority_list.focused = true,
            Field::AgentCli => self.agent_list.focused = true,
        }
    }
}

impl Modal for CreateTaskModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // In Notes field, pass Enter (newline) and Up/Down (line nav) to TextArea
        if matches!(self.current_field, Field::Notes) {
            if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
                self.notes_input.insert_newline();
                return Ok(None);
            }
            if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.notes_input.handle_key(key);
                return Ok(None);
            }
        }
        // In LinkUrl field, pass Enter (newline, up to max lines) and Up/Down to TextArea
        if matches!(self.current_field, Field::LinkUrl) {
            if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
                let current_lines = self.link_url_input.value.split('\n').count();
                if current_lines < MAX_LINK_LINES {
                    self.link_url_input.insert_newline();
                }
                return Ok(None);
            }
            if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.link_url_input.handle_key(key);
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
            let agent_cli = self
                .agent_list
                .selected_value()
                .copied()
                .unwrap_or(AgentCli::None);
            let notes = if self.notes_input.value.is_empty() {
                None
            } else {
                Some(self.notes_input.value.clone())
            };
            let links: Vec<TaskLink> = self
                .link_url_input
                .value
                .split('\n')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|url| TaskLink {
                    url: url.to_string(),
                    display_name: None,
                })
                .collect();

            return Ok(Some(Action::CreateTask {
                project_id: self.project_id.clone(),
                name: self.name_input.value.clone(),
                priority,
                agent_cli,
                notes,
                links,
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
                    Field::Notes => { self.notes_input.handle_key(key); },
                    Field::LinkUrl => { self.link_url_input.handle_key(key); },
                    Field::Priority => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.priority_list.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.priority_list.move_down(),
                        _ => {}
                    },
                    Field::AgentCli => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.agent_list.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.agent_list.move_down(),
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
            .title(" New Task ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        // Link field grows with line count: 1 line = height 3, up to 5 lines = height 7
        let link_line_count = self.link_url_input.value.split('\n').count().max(1);
        let link_height = (link_line_count as u16) + 2; // +2 for border

        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(link_height),
            Constraint::Length(7),
            Constraint::Length(5),
        ])
        .split(inner);

        self.name_input.render(frame, chunks[0]);
        self.notes_input.render(frame, chunks[1]);
        self.link_url_input.render(frame, chunks[2]);
        self.priority_list.render(frame, chunks[3]);
        self.agent_list.render(frame, chunks[4]);
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            Field::Name => self.name_input.insert_paste(text),
            Field::Notes => self.notes_input.insert_paste(text),
            Field::LinkUrl => {
                // Enforce max lines on paste
                let current_lines = self.link_url_input.value.split('\n').count();
                let remaining = MAX_LINK_LINES.saturating_sub(current_lines);
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                let paste_lines: Vec<&str> = normalized.split('\n').collect();
                // Allow first line always (appends to current line), then up to remaining new lines
                let limited: String = if paste_lines.len() <= remaining + 1 {
                    normalized
                } else {
                    paste_lines[..remaining + 1].join("\n")
                };
                self.link_url_input.insert_paste(&limited);
            }
            _ => {}
        }
    }
}
