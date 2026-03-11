use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct StatusBar;

impl StatusBar {
    pub fn render_main(frame: &mut Frame, area: Rect, error_msg: Option<&str>) {
        if let Some(err) = error_msg {
            let line = Line::from(vec![
                Span::styled(" ERROR: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(err, Style::default().fg(Color::Red)),
            ]);
            let bar = Paragraph::new(line).style(Style::default().bg(Color::Rgb(30, 30, 46)));
            frame.render_widget(bar, area);
            return;
        }

        let hints = Line::from(vec![
            Span::styled(" p", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Project ", Style::default().fg(Color::Gray)),
            Span::styled("n", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Task ", Style::default().fg(Color::Gray)),
            Span::styled("m", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Edit ", Style::default().fg(Color::Gray)),
            Span::styled("d", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Delete ", Style::default().fg(Color::Gray)),
            Span::styled("S", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Status ", Style::default().fg(Color::Gray)),
            Span::styled("L", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Link ", Style::default().fg(Color::Gray)),
            Span::styled("o", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Open ", Style::default().fg(Color::Gray)),
            Span::styled("f", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Filter ", Style::default().fg(Color::Gray)),
            Span::styled("A", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Action ", Style::default().fg(Color::Gray)),
            Span::styled("s", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Sort ", Style::default().fg(Color::Gray)),
            Span::styled("P", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" PR ", Style::default().fg(Color::Gray)),
            Span::styled("Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Attach ", Style::default().fg(Color::Gray)),
            Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Quit", Style::default().fg(Color::Gray)),
        ]);
        let bar = Paragraph::new(hints).style(Style::default().bg(Color::Rgb(30, 30, 46)));
        frame.render_widget(bar, area);
    }

    pub fn render_modal(frame: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled(" Esc/Ctrl+C", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel ", Style::default().fg(Color::Gray)),
            Span::styled("Ctrl+Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Confirm ", Style::default().fg(Color::Gray)),
            Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Next Field", Style::default().fg(Color::Gray)),
        ]);
        let bar = Paragraph::new(hints).style(Style::default().bg(Color::Rgb(30, 30, 46)));
        frame.render_widget(bar, area);
    }
}
