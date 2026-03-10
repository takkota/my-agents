use super::input::MultiSelectList;
use super::Modal;
use crate::action::Action;
use crate::domain::task::Status;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

pub struct FilterModal {
    status_list: MultiSelectList<Status>,
}

impl FilterModal {
    pub fn new(current_filter: Option<&[Status]>) -> Self {
        let items: Vec<(String, Status)> = Status::all()
            .iter()
            .map(|s| (format!("{} {}", s.symbol(), s), *s))
            .collect();
        let mut status_list = MultiSelectList::new("Filter by Status (Space to toggle)", items);
        status_list.focused = true;

        // Pre-select current filter
        if let Some(filter) = current_filter {
            for (_, status, checked) in &mut status_list.items {
                if filter.contains(status) {
                    *checked = true;
                }
            }
        }

        Self { status_list }
    }

    pub fn selected_statuses(&self) -> Option<Vec<Status>> {
        let selected: Vec<Status> = self
            .status_list
            .selected_values()
            .into_iter()
            .copied()
            .collect();
        if selected.is_empty() {
            None
        } else {
            Some(selected)
        }
    }
}

impl Modal for FilterModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Enter => Ok(Some(Action::ApplyAndCloseModal)),
            KeyCode::Up | KeyCode::Char('k') => {
                self.status_list.move_up();
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.status_list.move_down();
                Ok(None)
            }
            KeyCode::Char(' ') => {
                self.status_list.toggle();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Filter ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        self.status_list.render(frame, inner);
    }
}
