use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::InputMode;
use crate::ui::theme;

/// Render the message input area.
pub fn render(frame: &mut Frame, area: Rect, input_text: &str, mode: InputMode, focused: bool) {
    let border_style = if focused {
        theme::focused_border()
    } else {
        theme::unfocused_border()
    };

    let mode_hint = match mode {
        InputMode::Insert => " Reply ",
        InputMode::Normal => "",
    };

    let display = if input_text.is_empty() && mode == InputMode::Normal {
        "Press i to type a message...".dim().to_string()
    } else if mode == InputMode::Insert {
        format!("{input_text}\u{2588}") // block cursor
    } else {
        input_text.to_string()
    };

    let paragraph = Paragraph::new(display).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(ratatui::text::Span::styled(mode_hint, theme::title())),
    );

    frame.render_widget(paragraph, area);
}
