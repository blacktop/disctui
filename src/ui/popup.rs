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
        Line::from(vec![
            " ← / →     ".bold().cyan(),
            "Cycle left/right panels".dim(),
        ]),
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
        Line::from(" Discord Token ".bold().cyan()),
        Line::raw(""),
        Line::from("No token found in DISCTUI_TOKEN or the macOS Keychain.".dim()),
        Line::from("Enter your Discord user token to store it in Keychain and connect.".dim()),
        Line::raw(""),
        Line::from(vec![" Token: ".bold().cyan(), Span::raw(masked_input)]),
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
                .title(" Setup ".bold().cyan()),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}
