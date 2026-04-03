use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::model::ChannelDigest;
use crate::ui::theme;

/// Render the AI summary view (digest + TODO items).
pub fn render(
    frame: &mut Frame,
    area: Rect,
    digest: Option<&ChannelDigest>,
    selected_todo: Option<usize>,
    in_flight: bool,
    focused: bool,
) {
    let border_style = if focused {
        theme::focused_border()
    } else {
        theme::unfocused_border()
    };

    let lines = if in_flight {
        vec![Line::from(Span::styled(
            "Summarizing\u{2026}",
            theme::summary_header(),
        ))]
    } else if let Some(digest) = digest {
        build_digest_lines(digest, selected_todo)
    } else {
        vec![Line::from(Span::styled(
            "Press s to summarize messages",
            theme::dim(),
        ))]
    };

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(" Summary ".bold().magenta()),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn build_digest_lines(digest: &ChannelDigest, selected_todo: Option<usize>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled("Summary", theme::summary_header())),
        Line::raw(""),
    ];

    for summary_line in digest.summary.lines() {
        lines.push(Line::from(Span::raw(format!("  {summary_line}"))));
    }

    if !digest.todos.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Action Items",
            theme::summary_header(),
        )));
        lines.push(Line::raw(""));

        for (i, todo) in digest.todos.iter().enumerate() {
            let is_selected = selected_todo == Some(i);
            let bullet = if is_selected { "\u{25b6} " } else { "  " };
            let style = if is_selected {
                theme::selected_item()
            } else {
                theme::todo_bullet()
            };

            lines.push(Line::from(vec![
                Span::styled(format!("{bullet}{}: ", todo.author), style),
                Span::raw(todo.reason.clone()),
            ]));
            let snippet_suffix = if todo.message_id.is_empty() {
                String::new()
            } else {
                format!(" [{}]", todo.message_id)
            };
            lines.push(Line::from(Span::styled(
                format!("    \u{201c}{}\u{201d}{snippet_suffix}", todo.snippet),
                theme::dim(),
            )));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!(
            "#{} \u{2014} generated at {}",
            digest.channel_id, digest.generated_at
        ),
        theme::dim(),
    )));

    lines
}
