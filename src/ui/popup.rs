use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::ui::layout::centered_popup;
use crate::ui::theme;

/// Render the help overlay as a centered popup.
pub fn render_help(frame: &mut Frame, area: Rect) {
    let popup_area = centered_popup(60, 60, area);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let help_lines = vec![
        Line::from(" Keybindings ".bold().cyan()),
        Line::raw(""),
        Line::from(vec![" q         ".bold().cyan(), "Quit".dim()]),
        Line::from(vec![" ?         ".bold().cyan(), "Toggle help".dim()]),
        Line::from(vec![
            " 1/2/3     ".bold().cyan(),
            "Focus guilds/channels/messages".dim(),
        ]),
        Line::from(vec![" Tab       ".bold().cyan(), "Next pane".dim()]),
        Line::from(vec![" Shift+Tab ".bold().cyan(), "Previous pane".dim()]),
        Line::from(vec![" j / Down  ".bold().cyan(), "Move down".dim()]),
        Line::from(vec![" k / Up    ".bold().cyan(), "Move up".dim()]),
        Line::from(vec![" g         ".bold().cyan(), "Jump to top".dim()]),
        Line::from(vec![" G         ".bold().cyan(), "Jump to bottom".dim()]),
        Line::from(vec![" Enter     ".bold().cyan(), "Select / open".dim()]),
        Line::from(vec![" i         ".bold().cyan(), "Enter insert mode".dim()]),
        Line::from(vec![" Esc       ".bold().cyan(), "Exit insert mode".dim()]),
        Line::from(vec![" s         ".bold().cyan(), "Summarize unread".dim()]),
        Line::from(vec![" R         ".bold().cyan(), "Mark all read".dim()]),
        Line::from(vec![" m         ".bold().cyan(), "Show messages".dim()]),
        Line::from(vec![" Ctrl+C    ".bold().cyan(), "Force quit".dim()]),
    ];

    let paragraph = Paragraph::new(help_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::focused_border())
                .title(" Help ".bold().cyan()),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}
