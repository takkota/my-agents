use super::input::SelectList;
use super::Modal;
use crate::action::Action;
use crate::domain::task::PreviewUrl;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui::Frame;

pub struct SelectPreviewUrlModal {
    url_list: SelectList<String>,
}

impl SelectPreviewUrlModal {
    pub fn new(preview_urls: Vec<PreviewUrl>) -> Self {
        let items: Vec<(String, String)> = preview_urls
            .iter()
            .map(|p| {
                let label = format!("{} - {}", p.service_name, p.url);
                (label, p.url.clone())
            })
            .collect();
        let mut url_list = SelectList::new("Select Preview URL to Open", items);
        url_list.focused = true;
        Self { url_list }
    }
}

impl Modal for SelectPreviewUrlModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Enter => {
                if let Some(url) = self.url_list.selected_value().cloned() {
                    Ok(Some(Action::OpenLinkInBrowser { url }))
                } else {
                    Ok(Some(Action::CloseModal))
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.url_list.move_up();
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.url_list.move_down();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        self.url_list.render(frame, area);
    }
}
