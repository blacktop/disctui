use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{ConnectionState, FocusPane, InputMode};
use crate::model::ChannelKind;
use crate::ui::theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    mode: InputMode,
    focus: FocusPane,
    connection: ConnectionState,
    selected_channel: Option<(&str, ChannelKind)>,
    error_msg: Option<&str>,
) {
    let mode_span = match mode {
        InputMode::Normal => Span::styled(" NORMAL ", theme::mode_normal()),
        InputMode::Insert => Span::styled(" INSERT ", theme::mode_insert()),
    };

    let conn_span = match connection {
        ConnectionState::Connected => Span::styled(" \u{25cf} Connected ", theme::status_bar()),
        ConnectionState::Connecting => {
            Span::styled(" \u{25cb} Connecting\u{2026} ", theme::status_bar())
        }
        ConnectionState::Reconnecting => {
            Span::styled(" \u{25cb} Reconnecting\u{2026} ", theme::status_bar())
        }
        ConnectionState::Disconnected => {
            Span::styled(" \u{25cb} Disconnected ", theme::status_bar())
        }
        ConnectionState::MockTransport => Span::styled(" \u{25a0} Mock ", theme::status_bar()),
    };

    let focus_span = Span::styled(format!(" {} ", focus.label()), theme::status_bar());

    let channel_span = match selected_channel {
        Some((name, kind)) => {
            Span::styled(format!(" {} {name} ", kind.marker()), theme::status_bar())
        }
        _ => Span::styled(" -- ", theme::status_bar()),
    };

    let hints = Span::styled(" q:quit  ?:help  Tab:focus  i:insert ", theme::dim());

    let mut spans = vec![
        mode_span,
        Span::raw(" "),
        conn_span,
        Span::raw("\u{2502}"),
        focus_span,
        Span::raw("\u{2502}"),
        channel_span,
    ];

    if let Some(err) = error_msg {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(err.to_string(), theme::error()));
    }

    spans.push(Span::raw(" "));
    spans.push(hints);

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}
