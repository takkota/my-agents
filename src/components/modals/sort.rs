use super::input::SelectList;
use super::Modal;
use crate::action::Action;
use crate::components::task_tree::SortMode;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

pub struct SortModal {
    list: SelectList<SortMode>,
}

impl SortModal {
    pub fn new(current: SortMode) -> Self {
        let items = vec![
            ("Created (newest first)".to_string(), SortMode::CreatedDesc),
            ("Updated (newest first)".to_string(), SortMode::UpdatedDesc),
            ("Priority (highest first)".to_string(), SortMode::PriorityDesc),
        ];
        let mut list = SelectList::new("Sort Order", items);
        list.selected = match current {
            SortMode::CreatedDesc => 0,
            SortMode::UpdatedDesc => 1,
            SortMode::PriorityDesc => 2,
        };
        list.focused = true;
        Self { list }
    }

    pub fn selected_mode(&self) -> SortMode {
        self.list.selected_value().copied().unwrap_or(SortMode::CreatedDesc)
    }
}

impl Modal for SortModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)), // Cancel - discard changes
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.move_up();
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.list.move_down();
                Ok(None)
            }
            KeyCode::Enter => Ok(Some(Action::ApplyAndCloseModal)),
            _ => Ok(None),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Sort ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        self.list.render(frame, inner);
    }

    fn title(&self) -> &str {
        "Sort"
    }
}
