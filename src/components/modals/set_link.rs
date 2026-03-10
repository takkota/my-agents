use super::input::TextInput;
use super::Modal;
use crate::action::Action;
use crate::domain::task::TaskLink;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

enum Field {
    Url,
    DisplayName,
}

pub struct SetLinkModal {
    task_id: String,
    project_id: String,
    url_input: TextInput,
    name_input: TextInput,
    current_field: Field,
}

impl SetLinkModal {
    pub fn new(task_id: String, project_id: String) -> Self {
        let mut url_input = TextInput::new("URL");
        url_input.focused = true;
        let name_input = TextInput::new("Display Name (optional)");
        Self {
            task_id,
            project_id,
            url_input,
            name_input,
            current_field: Field::Url,
        }
    }

    fn next_field(&mut self) {
        self.url_input.focused = false;
        self.name_input.focused = false;
        self.current_field = match self.current_field {
            Field::Url => {
                self.name_input.focused = true;
                Field::DisplayName
            }
            Field::DisplayName => {
                self.url_input.focused = true;
                Field::Url
            }
        };
    }
}

impl Modal for SetLinkModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // Ctrl+Enter submits the form
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.url_input.value.is_empty() {
                return Ok(None);
            }
            let display_name = if self.name_input.value.is_empty() {
                None
            } else {
                Some(self.name_input.value.clone())
            };
            return Ok(Some(Action::UpdateTaskLink {
                task_id: self.task_id.clone(),
                project_id: self.project_id.clone(),
                link: TaskLink {
                    url: self.url_input.value.clone(),
                    display_name,
                },
            }));
        }
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Tab | KeyCode::BackTab => {
                self.next_field();
                Ok(None)
            }
            _ => {
                let input = match self.current_field {
                    Field::Url => &mut self.url_input,
                    Field::DisplayName => &mut self.name_input,
                };
                input.handle_key(key);
                Ok(None)
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Add Link ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
            .split(inner);

        self.url_input.render(frame, chunks[0]);
        self.name_input.render(frame, chunks[1]);
    }

    fn handle_paste(&mut self, text: &str) {
        match self.current_field {
            Field::Url => self.url_input.insert_paste(text),
            Field::DisplayName => self.name_input.insert_paste(text),
        }
    }
}
