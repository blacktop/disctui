use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::DiscordTokenPromptState;
use crate::ui::layout::centered_popup;
use crate::ui::theme;

/// Render the help overlay as a centered popup.
pub fn render_help(frame: &mut Frame, area: Rect) {
    let popup_area = centered_popup(60, 60, area);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let help_lines = vec![
        Line::from(Span::styled(" Keybindings ", theme::title())),
        Line::raw(""),
        Line::from(vec![
            Span::styled(" q         ", theme::selected_item()),
            "Quit".dim(),
        ]),
        Line::from(vec![
            Span::styled(" ?         ", theme::selected_item()),
            "Toggle help".dim(),
        ]),
        Line::from(vec![
            Span::styled(" 1/2/3     ", theme::selected_item()),
            "Focus guilds/channels/messages".dim(),
        ]),
        Line::from(vec![
            Span::styled(" Tab       ", theme::selected_item()),
            "Next pane".dim(),
        ]),
        Line::from(vec![
            Span::styled(" Shift+Tab ", theme::selected_item()),
            "Previous pane".dim(),
        ]),
        Line::from(vec![
            Span::styled(" ← / →     ", theme::selected_item()),
            "Cycle left/right panels".dim(),
        ]),
        Line::from(vec![
            Span::styled(" j / Down  ", theme::selected_item()),
            "Move down".dim(),
        ]),
        Line::from(vec![
            Span::styled(" k / Up    ", theme::selected_item()),
            "Move up".dim(),
        ]),
        Line::from(vec![
            Span::styled(" g         ", theme::selected_item()),
            "Jump to top".dim(),
        ]),
        Line::from(vec![
            Span::styled(" G         ", theme::selected_item()),
            "Jump to bottom".dim(),
        ]),
        Line::from(vec![
            Span::styled(" Enter     ", theme::selected_item()),
            "Select / open".dim(),
        ]),
        Line::from(vec![
            Span::styled(" i         ", theme::selected_item()),
            "Enter insert mode".dim(),
        ]),
        Line::from(vec![
            Span::styled(" Esc       ", theme::selected_item()),
            "Exit insert mode".dim(),
        ]),
        Line::from(vec![
            Span::styled(" s         ", theme::selected_item()),
            "Summarize unread".dim(),
        ]),
        Line::from(vec![
            Span::styled(" r         ", theme::selected_item()),
            "Refresh now".dim(),
        ]),
        Line::from(vec![
            Span::styled(" R         ", theme::selected_item()),
            "Mark all read".dim(),
        ]),
        Line::from(vec![
            Span::styled(" m         ", theme::selected_item()),
            "Show messages".dim(),
        ]),
        Line::from(vec![
            Span::styled(" Ctrl+C    ", theme::selected_item()),
            "Force quit".dim(),
        ]),
    ];

    let paragraph = Paragraph::new(help_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::focused_border())
                .title(Span::styled(" Help ", theme::title())),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}

/// Render the startup Discord token prompt.
pub fn render_discord_token_prompt(
    frame: &mut Frame,
    area: Rect,
    prompt: &DiscordTokenPromptState,
) {
    let popup_area = centered_popup(70, 40, area);
    frame.render_widget(Clear, popup_area);

    let masked_input = if prompt.input.is_empty() {
        "Paste Discord token and press Enter".dim().to_string()
    } else {
        format!("{}█", "*".repeat(prompt.input.chars().count()))
    };

    let mut lines = vec![
        Line::from(Span::styled(" Discord Token ", theme::title())),
        Line::raw(""),
        Line::from("No token found in DISCTUI_TOKEN or the macOS Keychain.".dim()),
        Line::from("Enter your Discord user token to store it in Keychain and connect.".dim()),
        Line::raw(""),
        Line::from(vec![
            Span::styled(" Token: ", theme::selected_item()),
            Span::raw(masked_input),
        ]),
        Line::raw(""),
        Line::from("Enter: save to Keychain and connect".dim()),
        Line::from("Esc: continue in mock mode".dim()),
    ];

    if let Some(error) = prompt.error.as_deref() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(error.to_string(), theme::error())));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::focused_border())
                .title(Span::styled(" Setup ", theme::title())),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}
