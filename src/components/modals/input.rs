use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Simple single-line text input field (UTF-8 safe, cursor tracks char indices)
#[derive(Debug, Clone)]
pub struct TextInput {
    pub value: String,
    /// Cursor position as character index (not byte index)
    pub cursor: usize,
    pub label: String,
    pub focused: bool,
}

impl TextInput {
    pub fn new(label: &str) -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            label: label.to_string(),
            focused: false,
        }
    }

    pub fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self.cursor = value.chars().count();
        self
    }

    fn byte_offset(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.byte_offset();
        self.value.insert(byte_pos, c);
        self.cursor += 1;
    }

    /// Insert a string (e.g. from paste), converting newlines to spaces
    pub fn insert_paste(&mut self, text: &str) {
        let sanitized = text.replace("\r\n", " ").replace(['\n', '\r'], " ");
        for c in sanitized.chars() {
            self.insert_char(c);
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_pos = self.byte_offset();
            self.value.remove(byte_pos);
        }
    }

    pub fn delete_forward_char(&mut self) {
        if self.cursor < self.value.chars().count() {
            let byte_pos = self.byte_offset();
            self.value.remove(byte_pos);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.value.chars().count() {
            self.cursor += 1;
        }
    }

    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor = self.value.chars().count();
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let display_value = if self.focused {
            let byte_pos = self.byte_offset();
            let before = &self.value[..byte_pos];
            let (cursor_char, after) = if byte_pos < self.value.len() {
                let ch = self.value[byte_pos..].chars().next().unwrap();
                let end = byte_pos + ch.len_utf8();
                (&self.value[byte_pos..end], &self.value[end..])
            } else {
                (" ", "")
            };
            Line::from(vec![
                Span::raw(before.to_string()),
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().bg(Color::White).fg(Color::Black),
                ),
                Span::raw(after.to_string()),
            ])
        } else {
            Line::from(self.value.as_str())
        };

        let input = Paragraph::new(display_value).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.label))
                .border_style(Style::default().fg(border_color)),
        );
        frame.render_widget(input, area);
    }
}

/// Selection list component
#[derive(Debug, Clone)]
pub struct SelectList<T: Clone> {
    pub items: Vec<(String, T)>,
    pub selected: usize,
    pub label: String,
    pub focused: bool,
}

impl<T: Clone> SelectList<T> {
    pub fn new(label: &str, items: Vec<(String, T)>) -> Self {
        Self {
            items,
            selected: 0,
            label: label.to_string(),
            focused: false,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected < self.items.len().saturating_sub(1) {
            self.selected += 1;
        }
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.items.get(self.selected).map(|(_, v)| v)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, (label, _))| {
                if i == self.selected {
                    Line::from(vec![
                        Span::styled(" > ", Style::default().fg(Color::Cyan)),
                        Span::styled(label.as_str(), Style::default().fg(Color::White)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw("   "),
                        Span::raw(label.as_str()),
                    ])
                }
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.label))
                .border_style(Style::default().fg(border_color)),
        );
        frame.render_widget(paragraph, area);
    }
}

/// Multi-select list with checkboxes
#[derive(Debug, Clone)]
pub struct MultiSelectList<T: Clone> {
    pub items: Vec<(String, T, bool)>,
    pub cursor: usize,
    pub label: String,
    pub focused: bool,
    pub filter_text: String,
}

impl<T: Clone> MultiSelectList<T> {
    pub fn new(label: &str, items: Vec<(String, T)>) -> Self {
        let items = items.into_iter().map(|(l, v)| (l, v, false)).collect();
        Self {
            items,
            cursor: 0,
            label: label.to_string(),
            focused: false,
            filter_text: String::new(),
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.filter_text.is_empty() {
            return (0..self.items.len()).collect();
        }
        let query = self.filter_text.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, (label, _, _))| label.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.filtered_indices().len().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    pub fn toggle(&mut self) {
        let indices = self.filtered_indices();
        if let Some(&idx) = indices.get(self.cursor) {
            self.items[idx].2 = !self.items[idx].2;
        }
    }

    pub fn selected_values(&self) -> Vec<&T> {
        self.items
            .iter()
            .filter(|(_, _, selected)| *selected)
            .map(|(_, v, _)| v)
            .collect()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let indices = self.filtered_indices();
        let mut lines: Vec<Line> = Vec::new();

        // Filter input line
        if self.focused {
            lines.push(Line::from(vec![
                Span::styled("Filter: ", Style::default().fg(Color::DarkGray)),
                Span::styled(self.filter_text.as_str(), Style::default().fg(Color::White)),
            ]));
        }

        for (display_idx, &real_idx) in indices.iter().enumerate() {
            let (label, _, checked) = &self.items[real_idx];
            let checkbox = if *checked { "[x]" } else { "[ ]" };
            let is_cursor = display_idx == self.cursor;

            if is_cursor {
                lines.push(Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{} {}", checkbox, label),
                        Style::default().fg(Color::White),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::raw(format!("{} {}", checkbox, label)),
                ]));
            }
        }

        // Calculate scroll offset to keep cursor visible
        // Available height = area height - 2 (borders)
        let visible_height = area.height.saturating_sub(2) as usize;
        // The cursor line index within `lines` is offset by 1 if filter line is shown
        let cursor_line = if self.focused {
            self.cursor + 1 // +1 for the filter line
        } else {
            self.cursor
        };
        let scroll_offset = if visible_height > 0 && cursor_line >= visible_height {
            (cursor_line - visible_height + 1) as u16
        } else {
            0
        };

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.label))
                    .border_style(Style::default().fg(border_color)),
            )
            .scroll((scroll_offset, 0));
        frame.render_widget(paragraph, area);
    }
}
