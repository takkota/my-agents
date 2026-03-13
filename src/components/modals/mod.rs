pub mod create_project;
pub mod create_task;
pub mod confirm_delete;
pub mod custom_prompt;
pub mod set_status;
pub mod set_link;
pub mod select_link;
pub mod select_preview_url;
pub mod edit_item;
pub mod filter;
pub mod sort;
pub mod settings;
pub mod input;

use crate::action::Action;
use crate::error::AppResult;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

pub trait Modal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>>;
    fn handle_paste(&mut self, _text: &str) {}
    fn render(&self, frame: &mut Frame, area: Rect);
}

/// Parse a comma-separated input string into a Vec of trimmed, non-empty strings.
pub fn parse_comma_separated(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Calculate a centered rect for modal overlay
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    centered_rect_with_max(percent_x, percent_y, 80, 30, area)
}

/// Calculate a centered rect with custom max dimensions
pub fn centered_rect_with_max(percent_x: u16, percent_y: u16, max_width: u16, max_height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let popup_height = area.height * percent_y / 100;
    let w = popup_width.min(max_width);
    let h = popup_height.min(max_height);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    Rect::new(area.x + x, area.y + y, w, h)
}
