use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};

use crate::model::ChannelDigest;
use crate::ui::theme;

/// Render the AI summary view (digest + action items).
pub fn render(
    frame: &mut Frame,
    area: Rect,
    digest: Option<&ChannelDigest>,
    selected_todo: Option<usize>,
    in_flight: bool,
) {
    let lines = if in_flight {
        vec![Line::from(Span::styled(
            "Summarizing\u{2026}",
            theme::summary_header(),
        ))]
    } else if let Some(digest) = digest {
        build_digest_lines(digest, selected_todo)
    } else {
        vec![Line::from(Span::styled(
            "Press s to summarize unread messages",
            theme::dim(),
        ))]
    };

    let paragraph = Paragraph::new(lines)
        .block(Block::new().padding(Padding::new(2, 2, 1, 0)))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn build_digest_lines(digest: &ChannelDigest, selected_todo: Option<usize>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(digest.headline.clone(), theme::unread_name())),
        Line::from(Span::styled(
            format!("generated {}", digest.generated_at),
            theme::dim(),
        )),
        Line::raw(""),
        Line::from(Span::styled("Summary", theme::summary_header())),
    ];
    lines.extend(
        digest
            .summary
            .lines()
            .map(|summary_line| Line::raw(summary_line.to_string())),
    );

    if digest.todos.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Nothing needs you here.",
            theme::dim(),
        )));
        return lines;
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Needs you", theme::attention())));
    for (index, todo) in digest.todos.iter().enumerate() {
        let is_selected = selected_todo == Some(index);
        let (bullet, author_style) = if is_selected {
            ("\u{25b8} ", theme::selected_item())
        } else {
            ("  ", theme::attention())
        };
        lines.push(Line::from(vec![
            Span::styled(bullet, theme::selected_item()),
            Span::styled(todo.author.clone(), author_style),
            Span::raw("  "),
            Span::styled(todo.reason.clone(), theme::dim()),
        ]));
        lines.push(Line::raw(format!("  \u{201c}{}\u{201d}", todo.snippet)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use crate::model::TodoItem;
    use crate::ui::summary::*;

    fn digest(todos: Vec<TodoItem>) -> ChannelDigest {
        ChannelDigest {
            channel_id: "c1".into(),
            headline: "Edition migration".into(),
            summary: "First line.\nSecond line.".into(),
            todos,
            generated_at: "10:40".into(),
        }
    }

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn digest_starts_with_headline_and_keeps_summary_lines() {
        let lines = text(&build_digest_lines(&digest(Vec::new()), None));
        assert_eq!(lines[0], "Edition migration");
        assert_eq!(lines[1], "generated 10:40");
        assert!(lines.contains(&"First line.".to_string()));
        assert!(lines.contains(&"Second line.".to_string()));
        assert_eq!(
            lines.last().map(String::as_str),
            Some("Nothing needs you here.")
        );
    }

    #[test]
    fn selected_action_item_gets_the_cursor() {
        let todo = |author: &str| TodoItem {
            author: author.into(),
            snippet: "thoughts?".into(),
            reason: "Direct question to you".into(),
            message_id: "m12".into(),
        };
        let lines = text(&build_digest_lines(
            &digest(vec![todo("dave"), todo("eve")]),
            Some(1),
        ));
        assert!(lines.contains(&"  dave  Direct question to you".to_string()));
        assert!(lines.contains(&"\u{25b8} eve  Direct question to you".to_string()));
        assert!(lines.contains(&"  \u{201c}thoughts?\u{201d}".to_string()));
    }
}
