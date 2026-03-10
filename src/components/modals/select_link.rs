use super::input::SelectList;
use super::Modal;
use crate::action::Action;
use crate::domain::task::TaskLink;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui::Frame;

pub struct SelectLinkModal {
    link_list: SelectList<String>,
}

impl SelectLinkModal {
    pub fn new(links: Vec<TaskLink>) -> Self {
        let items: Vec<(String, String)> = links
            .iter()
            .map(|l| {
                let label = format!("{} - {}", l.display(), l.url);
                (label, l.url.clone())
            })
            .collect();
        let mut link_list = SelectList::new("Select Link to Open", items);
        link_list.focused = true;
        Self { link_list }
    }
}

impl Modal for SelectLinkModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Enter => {
                if let Some(url) = self.link_list.selected_value().cloned() {
                    Ok(Some(Action::OpenLinkInBrowser { url }))
                } else {
                    Ok(Some(Action::CloseModal))
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.link_list.move_up();
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.link_list.move_down();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        self.link_list.render(frame, area);
    }
}
