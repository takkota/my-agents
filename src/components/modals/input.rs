use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

    pub fn delete_to_start(&mut self) {
        if self.cursor > 0 {
            let byte_pos = self.byte_offset();
            self.value.drain(..byte_pos);
            self.cursor = 0;
        }
    }

    pub fn delete_to_end(&mut self) {
        let byte_pos = self.byte_offset();
        self.value.truncate(byte_pos);
    }

    /// Handle common text editing key events. Returns true if the key was handled.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => self.delete_to_start(),
                KeyCode::Char('k') => self.delete_to_end(),
                _ => return false,
            }
            return true;
        }
        match key.code {
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Backspace => self.delete_char(),
            KeyCode::Delete => self.delete_forward_char(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.move_to_start(),
            KeyCode::End => self.move_to_end(),
            _ => return false,
        }
        true
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

/// Multi-line text input field (UTF-8 safe, supports Shift+Enter for newlines)
#[derive(Debug, Clone)]
pub struct TextArea {
    pub value: String,
    /// Cursor position as character index (not byte index) within the entire value
    pub cursor: usize,
    pub label: String,
    pub focused: bool,
}

impl TextArea {
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

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Insert a string (e.g. from paste), preserving newlines
    pub fn insert_paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for c in normalized.chars() {
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

    /// Get (line_index, column_index) of the current cursor position
    fn cursor_line_col(&self) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        for (i, ch) in self.value.chars().enumerate() {
            if i == self.cursor {
                return (line, col);
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Get the lines of the value as (start_char_index, char_count) pairs
    fn line_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = 0;
        let mut count = 0;
        for ch in self.value.chars() {
            if ch == '\n' {
                ranges.push((start, count));
                start = start + count + 1;
                count = 0;
            } else {
                count += 1;
            }
        }
        ranges.push((start, count));
        ranges
    }

    pub fn move_up(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        let ranges = self.line_ranges();
        let prev = &ranges[line - 1];
        let target_col = col.min(prev.1);
        self.cursor = prev.0 + target_col;
    }

    pub fn move_down(&mut self) {
        let (line, col) = self.cursor_line_col();
        let ranges = self.line_ranges();
        if line >= ranges.len() - 1 {
            return;
        }
        let next = &ranges[line + 1];
        let target_col = col.min(next.1);
        self.cursor = next.0 + target_col;
    }

    pub fn move_to_line_start(&mut self) {
        let (line, _) = self.cursor_line_col();
        let ranges = self.line_ranges();
        self.cursor = ranges[line].0;
    }

    pub fn move_to_line_end(&mut self) {
        let (line, _) = self.cursor_line_col();
        let ranges = self.line_ranges();
        self.cursor = ranges[line].0 + ranges[line].1;
    }

    pub fn delete_to_line_start(&mut self) {
        let (line, col) = self.cursor_line_col();
        if col > 0 {
            let ranges = self.line_ranges();
            let line_start_byte: usize = self.value.char_indices()
                .nth(ranges[line].0)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let cursor_byte = self.byte_offset();
            self.value.drain(line_start_byte..cursor_byte);
            self.cursor -= col;
        }
    }

    pub fn delete_to_line_end(&mut self) {
        let cursor_byte = self.byte_offset();
        // Find the next newline or end of string
        let end_byte = self.value[cursor_byte..]
            .find('\n')
            .map(|pos| cursor_byte + pos)
            .unwrap_or(self.value.len());
        self.value.drain(cursor_byte..end_byte);
    }

    /// Handle key events. Returns true if the key was handled.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Shift+Enter inserts newline
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) {
            self.insert_newline();
            return true;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => self.delete_to_line_start(),
                KeyCode::Char('k') => self.delete_to_line_end(),
                _ => return false,
            }
            return true;
        }
        match key.code {
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Backspace => self.delete_char(),
            KeyCode::Delete => self.delete_forward_char(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Home => self.move_to_line_start(),
            KeyCode::End => self.move_to_line_end(),
            _ => return false,
        }
        true
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let text_lines: Vec<&str> = self.value.split('\n').collect();

        let display_lines: Vec<Line> = if self.focused {
            let (cursor_line, cursor_col) = self.cursor_line_col();
            text_lines
                .iter()
                .enumerate()
                .map(|(line_idx, line_text)| {
                    if line_idx == cursor_line {
                        let chars: Vec<char> = line_text.chars().collect();
                        let before: String = chars[..cursor_col].iter().collect();
                        if cursor_col < chars.len() {
                            let cursor_char = chars[cursor_col].to_string();
                            let after: String = chars[cursor_col + 1..].iter().collect();
                            Line::from(vec![
                                Span::raw(before),
                                Span::styled(
                                    cursor_char,
                                    Style::default().bg(Color::White).fg(Color::Black),
                                ),
                                Span::raw(after),
                            ])
                        } else {
                            Line::from(vec![
                                Span::raw(before),
                                Span::styled(
                                    " ",
                                    Style::default().bg(Color::White).fg(Color::Black),
                                ),
                            ])
                        }
                    } else {
                        Line::from(line_text.to_string())
                    }
                })
                .collect()
        } else {
            text_lines.iter().map(|l| Line::from(l.to_string())).collect()
        };

        // Scroll to keep cursor visible
        let visible_height = area.height.saturating_sub(2) as usize;
        let (cursor_line, _) = self.cursor_line_col();
        let scroll_offset = if visible_height > 0 && cursor_line >= visible_height {
            (cursor_line - visible_height + 1) as u16
        } else {
            0
        };

        let hint = if self.focused { " (Shift+Enter: newline) " } else { "" };
        let input = Paragraph::new(display_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} {}", self.label, hint))
                    .border_style(Style::default().fg(border_color)),
            )
            .scroll((scroll_offset, 0));
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
