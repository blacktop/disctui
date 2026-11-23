use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::InputMode;
use crate::model::ChannelKind;
use crate::ui::theme;

const PROMPT: &str = " \u{203a} ";

/// Render the composer: a hairline, then the prompt and draft.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    input_text: &str,
    mode: InputMode,
    channel: Option<(&str, ChannelKind)>,
) {
    let paragraph = Paragraph::new(composer_line(input_text, mode, channel))
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(theme::divider()),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn composer_line(
    input_text: &str,
    mode: InputMode,
    channel: Option<(&str, ChannelKind)>,
) -> Line<'static> {
    let prompt_style = match mode {
        InputMode::Insert => theme::title(),
        InputMode::Normal => theme::dim(),
    };
    let mut spans = vec![Span::styled(PROMPT, prompt_style)];

    match mode {
        InputMode::Insert => {
            spans.push(Span::raw(input_text.to_string()));
            spans.push(Span::styled("\u{2588}", theme::title()));
        }
        InputMode::Normal if input_text.is_empty() => {
            let placeholder = match channel {
                Some((name, kind)) => format!("Message {}{name}  (i to write)", kind.marker()),
                None => "Open a channel to write".to_string(),
            };
            spans.push(Span::styled(placeholder, theme::dim()));
        }
        InputMode::Normal => spans.push(Span::raw(input_text.to_string())),
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use crate::ui::input::*;

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn placeholder_names_the_open_channel() {
        let line = composer_line("", InputMode::Normal, Some(("general", ChannelKind::Text)));
        assert_eq!(text(&line), " \u{203a} Message #general  (i to write)");

        let dm = composer_line(
            "",
            InputMode::Normal,
            Some(("alice", ChannelKind::DirectMessage)),
        );
        assert!(text(&dm).contains("Message @alice"));
    }

    #[test]
    fn placeholder_without_a_channel_says_what_to_do() {
        let line = composer_line("", InputMode::Normal, None);
        assert!(text(&line).contains("Open a channel to write"));
    }

    #[test]
    fn insert_mode_shows_draft_with_cursor() {
        let line = composer_line("hello", InputMode::Insert, None);
        assert_eq!(text(&line), " \u{203a} hello\u{2588}");
    }

    #[test]
    fn unsent_draft_stays_visible_in_normal_mode() {
        let line = composer_line("draft", InputMode::Normal, None);
        assert_eq!(text(&line), " \u{203a} draft");
    }
}
