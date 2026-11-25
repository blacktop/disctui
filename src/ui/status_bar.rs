use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, ConnectionState, FocusPane, InputMode, MessagePaneView};
use crate::ui::theme;

/// The footer: current mode, the keys that matter right now, and the connection.
pub struct Footer {
    mode: InputMode,
    focus: FocusPane,
    summary_view: bool,
    digest_view: bool,
    /// The sign-in prompt is up; it lists its own keys.
    signing_in: bool,
    account: Option<String>,
    connection: ConnectionState,
    username: String,
    error: Option<String>,
    notice: Option<&'static str>,
}

impl Footer {
    pub fn from_app(app: &App) -> Self {
        Self {
            mode: app.input_mode,
            focus: app.focus,
            summary_view: app.message_pane_view == MessagePaneView::Summary,
            digest_view: app.digest.visible,
            signing_in: app.has_sign_in(),
            account: app.browser_account().map(str::to_owned),
            connection: app.connection_state,
            username: app.current_username.clone(),
            error: app.status_error().map(str::to_string),
            notice: app.desktop_notice(),
        }
    }

    fn mode_chip(&self) -> Span<'static> {
        if self.account.is_some() {
            return Span::styled(" ACCOUNT ", theme::mode_chip());
        }
        if self.signing_in {
            return Span::styled(" SIGN IN ", theme::mode_chip());
        }
        if self.digest_view {
            return Span::styled(" DIGEST ", theme::mode_chip());
        }
        match self.mode {
            InputMode::Insert => Span::styled(" INSERT ", theme::brand_chip()),
            InputMode::Normal if self.summary_view => Span::styled(" SUMMARY ", theme::mode_chip()),
            InputMode::Normal => Span::styled(" NORMAL ", theme::mode_chip()),
        }
    }

    /// Keys worth showing for the current mode and focused pane.
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.signing_in {
            return Vec::new();
        }
        if self.digest_view {
            return vec![
                ("j/k", "move"),
                ("enter", "open"),
                ("esc", "back"),
                ("S", "regenerate"),
                ("?", "help"),
            ];
        }
        let mut hints: Vec<(&'static str, &'static str)> = match (self.mode, self.focus) {
            (InputMode::Insert, _) => {
                return vec![
                    ("enter", "send"),
                    ("shift+enter", "newline"),
                    ("esc", "done"),
                ];
            }
            (InputMode::Normal, FocusPane::Guilds) => {
                vec![("j/k", "move"), ("enter", "open"), ("R", "mark all read")]
            }
            (InputMode::Normal, FocusPane::Channels) => {
                vec![("j/k", "move"), ("enter", "open"), ("x", "close dm")]
            }
            (InputMode::Normal, FocusPane::Messages | FocusPane::Input) if self.summary_view => {
                vec![("j/k", "item"), ("enter", "jump"), ("m", "messages")]
            }
            (InputMode::Normal, FocusPane::Messages | FocusPane::Input) => {
                vec![
                    ("j/k", "scroll"),
                    ("J/K", "message"),
                    ("c", "copy"),
                    ("o", "link"),
                    ("i", "write"),
                    ("s", "summarize"),
                ]
            }
        };
        // Least important last: hints are dropped from the end when space runs out.
        hints.extend([("?", "help"), ("q", "quit"), ("tab", "pane")]);
        match self.connection {
            ConnectionState::Disconnected => hints.insert(0, ("C", "retry connection")),
            ConnectionState::MockTransport => {
                hints.push(("C", "connect"));
            }
            ConnectionState::Connected
            | ConnectionState::SessionExpired
            | ConnectionState::Connecting
            | ConnectionState::Reconnecting => {}
        }
        hints
    }

    /// Mode chip plus as many hints as fit in `max_width` columns.
    fn left(&self, max_width: usize) -> Line<'static> {
        let mut spans = vec![self.mode_chip(), Span::raw(" ")];
        if let Some(error) = &self.error {
            spans.push(Span::styled(error.clone(), theme::error()));
            return Line::from(spans);
        }
        if let Some(notice) = self.notice {
            spans.push(Span::styled(notice, theme::success()));
            return Line::from(spans);
        }
        let mut used: usize = spans.iter().map(Span::width).sum();
        for (key, label) in self.hints() {
            let hint = [
                Span::styled(key, theme::key()),
                Span::styled(format!(" {label}  "), theme::dim()),
            ];
            let width: usize = hint.iter().map(Span::width).sum();
            if used + width > max_width {
                break;
            }
            used += width;
            spans.extend(hint);
        }
        Line::from(spans)
    }

    fn right(&self) -> Line<'static> {
        if let Some(account) = &self.account {
            return Line::styled(format!("● {account} "), theme::success());
        }
        let state = match self.connection {
            ConnectionState::Connected => Span::styled("\u{25cf} connected", theme::success()),
            ConnectionState::Connecting => {
                Span::styled("\u{25cb} connecting\u{2026}", theme::attention())
            }
            ConnectionState::Reconnecting => {
                Span::styled("\u{25cb} reconnecting\u{2026}", theme::attention())
            }
            ConnectionState::SessionExpired => Span::styled("○ session expired", theme::error()),
            ConnectionState::Disconnected => Span::styled("\u{25cb} disconnected", theme::error()),
            ConnectionState::MockTransport => Span::styled("\u{25a0} sample data", theme::dim()),
        };
        let mut spans = vec![state];
        if self.connection == ConnectionState::Connected && !self.username.is_empty() {
            spans.push(Span::styled(format!(" {}", self.username), theme::dim()));
        }
        spans.push(Span::raw(" "));
        Line::from(spans)
    }
}

pub fn render(frame: &mut Frame, area: Rect, footer: &Footer) {
    let right = footer.right();
    let right_width = u16::try_from(right.width()).unwrap_or(u16::MAX);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_width)]).areas(area);

    frame.render_widget(
        Paragraph::new(footer.left(usize::from(left_area.width))).style(theme::bar()),
        left_area,
    );
    frame.render_widget(Paragraph::new(right).style(theme::bar()), right_area);
}

#[cfg(test)]
mod tests {
    use crate::ui::status_bar::*;

    fn footer(mode: InputMode, focus: FocusPane, connection: ConnectionState) -> Footer {
        Footer {
            mode,
            focus,
            summary_view: false,
            digest_view: false,
            signing_in: false,
            account: None,
            connection,
            username: "ferris".into(),
            error: None,
            notice: None,
        }
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn insert_mode_shows_only_composer_keys() {
        let footer = footer(
            InputMode::Insert,
            FocusPane::Input,
            ConnectionState::Connected,
        );
        let left = text(&footer.left(200));
        assert!(left.starts_with(" INSERT "));
        assert!(left.contains("enter send"));
        assert!(!left.contains("quit"));
    }

    #[test]
    fn hints_follow_the_focused_pane() {
        let channels = footer(
            InputMode::Normal,
            FocusPane::Channels,
            ConnectionState::Connected,
        );
        assert!(text(&channels.left(200)).contains("x close dm"));

        let messages = footer(
            InputMode::Normal,
            FocusPane::Messages,
            ConnectionState::Connected,
        );
        let left = text(&messages.left(200));
        assert!(left.contains("s summarize"));
        assert!(!left.contains("close dm"));
    }

    #[test]
    fn connect_hint_appears_only_when_offline() {
        let online = footer(
            InputMode::Normal,
            FocusPane::Messages,
            ConnectionState::Connected,
        );
        assert!(!text(&online.left(200)).contains("C connect"));

        let offline = footer(
            InputMode::Normal,
            FocusPane::Messages,
            ConnectionState::Disconnected,
        );
        assert!(text(&offline.left(200)).contains("C retry connection"));
        assert!(text(&offline.right()).contains("disconnected"));
    }

    #[test]
    fn hints_that_do_not_fit_are_dropped_whole() {
        let footer = footer(
            InputMode::Normal,
            FocusPane::Messages,
            ConnectionState::Disconnected,
        );
        let narrow = text(&footer.left(45));
        assert!(narrow.len() <= 45, "{narrow:?}");
        assert!(narrow.contains("C retry connection"));
        assert!(!narrow.contains("? help"));
        assert!(!narrow.contains("C connect"));
        assert!(narrow.ends_with("  "), "no partial hint: {narrow:?}");

        assert_eq!(text(&footer.left(0)), " NORMAL  ");
    }

    #[test]
    fn error_replaces_hints() {
        let mut footer = footer(
            InputMode::Normal,
            FocusPane::Messages,
            ConnectionState::Connected,
        );
        footer.error = Some("failed to send".into());
        let left = text(&footer.left(200));
        assert!(left.contains("failed to send"));
        assert!(!left.contains("summarize"));
    }

    #[test]
    fn username_shows_only_when_connected() {
        let connected = footer(
            InputMode::Normal,
            FocusPane::Guilds,
            ConnectionState::Connected,
        );
        assert!(text(&connected.right()).contains("connected ferris"));

        let mock = footer(
            InputMode::Normal,
            FocusPane::Guilds,
            ConnectionState::MockTransport,
        );
        let right = text(&mock.right());
        assert!(right.contains("sample data"));
        assert!(!right.contains("ferris"));
    }
}
