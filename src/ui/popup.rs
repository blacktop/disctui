use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph, Wrap};

use crate::app::{BrowserPhase, SignInMode, SignInState, WebStage};
use crate::ui::theme;

const KEY_COLUMN_WIDTH: usize = 12;
const SIGN_IN_TITLE: &str = "Sign in to Discord";
const ACCOUNT_TITLE: &str = "Account and server list only";
/// On macOS the first prompt leads with the sign-in that reaches messages.
#[cfg(target_os = "macos")]
const CONNECT_TITLE: &str = "Connect to Discord";
#[cfg(target_os = "macos")]
const CONNECT_HEADLINE: &str = "Sign in on Discord's own page in a window beside this one. Scan its QR code or type your \
     password, and finish any CAPTCHA there.";
#[cfg(target_os = "macos")]
const CONNECT_NOTE: &str =
    "disctui saves the session in this Mac's Keychain and reuses it next time.";
/// What Discord's browser authorization does and does not give disctui.
#[cfg(not(target_os = "macos"))]
const BROWSER_SCOPE_NOTE: &str =
    "This shows your name and servers. Channels and DMs require a message token.";
const BROWSER_NEXT_STEP_NOTE: &str =
    "Select a server to view its access requirements. Channels and DMs require a message token.";
const BROWSER_WIDTH: u16 = 58;
const RISK_WARNING: &str = "Unofficial client: a user token can put your account at risk.";
/// Border and padding columns of a popup, both sides together.
const POPUP_CHROME_WIDTH: u16 = 4;
const POPUP_CHROME_HEIGHT: u16 = 2;

/// Keybindings shown in the help popup, grouped by what they act on.
const HELP_SECTIONS: [(&str, &[(&str, &str)]); 4] = [
    (
        "Move",
        &[
            ("j / k", "Down / up"),
            ("g / G", "Top / bottom"),
            ("ctrl+d / u", "Half page down / up"),
            ("tab", "Next pane (shift+tab: previous)"),
            ("1 / 2 / 3", "Servers / channels / messages"),
            ("\u{2190} / \u{2192}", "Pane left / right"),
            ("enter", "Open the selected item"),
            ("1 then g", "Select Digest at the top of the rail"),
        ],
    ),
    (
        "Read and write",
        &[
            ("J / K", "Select newer / older message"),
            ("c / o", "Copy message text / open a web link"),
            ("i", "Write a message (esc when done)"),
            ("s", "Summarize unread messages"),
            ("m", "Back to messages"),
            ("R", "Mark everything read"),
            ("r", "Refresh now"),
        ],
    ),
    ("Direct messages", &[("x", "Close the selected DM")]),
    (
        "Session",
        &[
            ("Shift+C", "Connect or reconnect"),
            ("?", "Toggle this help"),
            ("q", "Quit (ctrl+c also works)"),
        ],
    ),
];

fn popup_block(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::popup_border())
        .title(Span::styled(format!(" {title} "), theme::title()))
        .padding(Padding::horizontal(1))
}

/// A popup of at most `width` x `height`, centered and clamped to the screen.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, (section, bindings)) in HELP_SECTIONS.iter().enumerate() {
        if index > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::from(Span::styled(*section, theme::dim())));
        for (key, description) in *bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<KEY_COLUMN_WIDTH$}"), theme::selected_item()),
                Span::raw(*description),
            ]));
        }
    }
    lines
}

/// Draw `lines` in a titled popup just tall enough to hold them.
fn render_popup(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    title: &'static str,
    lines: Vec<Line<'static>>,
) {
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(POPUP_CHROME_HEIGHT);
    let popup_area = centered(area, width, height);

    frame.render_widget(Clear, popup_area);
    frame.render_widget(Paragraph::new(lines).block(popup_block(title)), popup_area);
}

/// Render the help overlay as a centered popup.
pub fn render_help(frame: &mut Frame, area: Rect) {
    render_popup(frame, area, 52, "Keys", help_lines());
}

pub fn render_link_picker(
    frame: &mut Frame,
    area: Rect,
    picker: &mut crate::message_actions::LinkPicker,
) {
    picker.confirm_visible = area.height >= 4 && area.width >= 20;
    if !picker.confirm_visible {
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new("Resize to choose a link"), area);
        return;
    }
    let shown = usize::from(area.height.saturating_sub(3)).min(10);
    let first = picker.selected.saturating_sub(shown.saturating_sub(1));
    let mut lines: Vec<_> = picker
        .links
        .iter()
        .enumerate()
        .skip(first)
        .take(shown)
        .map(|(index, url)| {
            let (marker, style) = if index == picker.selected {
                ("›", theme::selected_item())
            } else {
                (" ", theme::dim())
            };
            Line::styled(format!("{marker} {url}"), style)
        })
        .collect();
    lines.push(Line::from("j/k select · Enter open · Esc cancel"));
    render_popup(frame, area, 90, "Open link", lines);
}

fn key_hint(key: &'static str, label: &'static str) -> [Span<'static>; 2] {
    [
        Span::styled(key, theme::key()),
        Span::styled(format!(" {label}   "), theme::dim()),
    ]
}

fn wrapped(text: &str, width: u16, style: Style) -> Vec<Line<'static>> {
    textwrap::wrap(text, usize::from(width))
        .into_iter()
        .map(|line| Line::from(Span::styled(line.into_owned(), style)))
        .collect()
}

fn token_entry_lines(input: &str, error: Option<&str>) -> Vec<Line<'static>> {
    let token = if input.is_empty() {
        Span::styled("paste your Discord token", theme::dim())
    } else {
        Span::raw(format!(
            "{}\u{2588}",
            "\u{2022}".repeat(input.chars().count())
        ))
    };

    let mut lines = vec![
        Line::raw("Paste a Discord user token. It is saved to the macOS Keychain."),
        Line::raw(""),
        Line::from(vec![Span::styled("Token  ", theme::selected_item()), token]),
        Line::raw(""),
    ];
    if let Some(error) = error {
        lines.push(Line::from(Span::styled(error.to_string(), theme::error())));
        lines.push(Line::raw(""));
    }
    lines.push(key_line(&[("enter", "save and connect"), ("esc", "back")]));
    lines.push(Line::from(Span::styled(RISK_WARNING, theme::dim())));
    lines
}

fn render_token_entry(frame: &mut Frame, area: Rect, input: &str, error: Option<&str>) {
    let lines = token_entry_lines(input, error);
    // Leave two spare rows so a long error can wrap.
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let popup_area = centered(area, 68, height);

    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(popup_block(SIGN_IN_TITLE))
            .wrap(Wrap { trim: false }),
        popup_area,
    );
}

/// One row of key hints.
fn key_line(hints: &[(&'static str, &'static str)]) -> Line<'static> {
    Line::from(
        hints
            .iter()
            .flat_map(|(key, label)| key_hint(key, label))
            .collect::<Vec<_>>(),
    )
}

/// Title and text of the account screen: what the browser sign-in is doing,
/// what it is good for, the last error, and the keys that work right now.
fn browser_lines(
    phase: &BrowserPhase,
    error: Option<&str>,
    width: u16,
) -> (&'static str, Vec<Line<'static>>) {
    let other_ways = [("t", "paste a token")];
    let leave = [("esc", "sample data"), ("q", "quit")];
    let quit = [("q", "quit")];
    let (title, headline, note, keys) = match phase {
        BrowserPhase::Restore => (
            ACCOUNT_TITLE,
            "Checking your saved session\u{2026}".to_owned(),
            None,
            vec![key_line(&quit)],
        ),
        #[cfg(target_os = "macos")]
        BrowserPhase::Ready => (
            CONNECT_TITLE,
            CONNECT_HEADLINE.to_owned(),
            Some(CONNECT_NOTE),
            vec![
                key_line(&[("enter", "connect messages")]),
                key_line(&[("b", "account and servers only")]),
                key_line(&other_ways),
                key_line(&leave),
            ],
        ),
        #[cfg(not(target_os = "macos"))]
        BrowserPhase::Ready => (
            ACCOUNT_TITLE,
            "Continue in your browser to sign in with Discord, including with a passkey."
                .to_owned(),
            Some(BROWSER_SCOPE_NOTE),
            vec![
                key_line(&[("enter", "open browser")]),
                key_line(&other_ways),
                key_line(&leave),
            ],
        ),
        BrowserPhase::AccountReady => (
            ACCOUNT_TITLE,
            "Continue in your browser to authorize access to your account and server list."
                .to_owned(),
            None,
            vec![key_line(&[("enter", "open browser")]), key_line(&leave)],
        ),
        BrowserPhase::Waiting => (
            ACCOUNT_TITLE,
            "Waiting for approval in your browser\u{2026}".to_owned(),
            None,
            vec![key_line(&[("esc", "cancel"), ("q", "quit")])],
        ),
        BrowserPhase::SignedIn(account) => {
            let mut keys = Vec::new();
            let note = if account.guilds.is_empty() {
                "This account belongs to no servers. Channels and DMs require a message token."
            } else {
                keys.push(key_line(&[("j/k", "server"), ("enter", "open")]));
                BROWSER_NEXT_STEP_NOTE
            };
            keys.extend([
                key_line(&[("t", "paste a token")]),
                key_line(&[("s", "sign out")]),
                key_line(&leave),
            ]);
            (
                ACCOUNT_TITLE,
                format!("Signed in as {}", account.name),
                Some(note),
                keys,
            )
        }
        BrowserPhase::SigningOut(_) => (
            ACCOUNT_TITLE,
            "Signing out\u{2026}".to_owned(),
            None,
            vec![key_line(&quit)],
        ),
    };

    let mut lines = wrapped(&headline, width, theme::unread_name());
    if let Some(note) = note {
        lines.push(Line::default());
        lines.extend(wrapped(note, width, theme::dim()));
    }
    if let Some(error) = error {
        lines.push(Line::default());
        lines.extend(wrapped(error, width, theme::error()));
    }
    lines.push(Line::default());
    lines.extend(keys);
    (title, lines)
}

fn render_browser(frame: &mut Frame, area: Rect, phase: &BrowserPhase, error: Option<&str>) {
    let width = BROWSER_WIDTH.min(area.width);
    let text_width = width.saturating_sub(POPUP_CHROME_WIDTH).max(1);
    let (title, lines) = browser_lines(phase, error, text_width);
    render_popup(frame, area, width, title, lines);
}

/// The sign-in window runs beside the terminal, so the prompt says what it is
/// doing and how to stop it.
fn web_sign_in_lines(stage: WebStage, error: Option<&str>, width: u16) -> Vec<Line<'static>> {
    let mut lines = wrapped(
        "Discord's own sign-in window is open. Finish there, including any CAPTCHA.",
        width,
        theme::dim(),
    );
    lines.push(Line::default());
    lines.push(Line::styled(stage.label().to_string(), theme::attention()));
    if let Some(error) = error {
        lines.push(Line::default());
        lines.extend(wrapped(error, width, theme::error()));
    }
    lines.push(Line::default());
    lines.push(Line::styled("Esc  Cancel".to_string(), theme::dim()));
    lines
}

fn render_web_sign_in(frame: &mut Frame, area: Rect, stage: WebStage, error: Option<&str>) {
    let width = BROWSER_WIDTH.min(area.width);
    let text_width = width.saturating_sub(POPUP_CHROME_WIDTH).max(1);
    let lines = web_sign_in_lines(stage, error, text_width);
    render_popup(frame, area, width, "Connect messages", lines);
}

/// Render the sign-in prompt: the browser account screen, the sign-in
/// window's progress, or the token field.
pub fn render_sign_in(frame: &mut Frame, area: Rect, sign_in: &SignInState) {
    let error = sign_in.error.as_deref();
    match &sign_in.mode {
        SignInMode::Browser(phase) => render_browser(frame, area, phase, error),
        SignInMode::Web(stage) => render_web_sign_in(frame, area, *stage, error),
        SignInMode::Token { input } => render_token_entry(frame, area, input, error),
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::popup::*;

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
    fn popup_is_centered_and_never_larger_than_the_screen() {
        let area = Rect::new(0, 0, 100, 40);
        let popup = centered(area, 52, 20);
        assert_eq!((popup.width, popup.height), (52, 20));
        assert_eq!((popup.x, popup.y), (24, 10));

        let small = Rect::new(0, 0, 30, 8);
        let popup = centered(small, 52, 20);
        assert_eq!(popup, small);
    }

    #[test]
    fn help_lists_every_section_with_aligned_keys() {
        let lines = text(&help_lines());
        for section in ["Move", "Read and write", "Direct messages", "Session"] {
            assert!(lines.contains(&section.to_string()), "missing {section}");
        }
        assert!(lines.contains(&"s           Summarize unread messages".to_string()));
    }

    #[test]
    fn the_sign_in_window_prompt_shows_its_stage_and_how_to_stop() {
        let lines = text(&web_sign_in_lines(WebStage::Waiting, None, 60));
        assert!(lines.contains(&"Waiting for Discord".to_string()));
        assert!(lines.contains(&"Esc  Cancel".to_string()));

        let failed = text(&web_sign_in_lines(
            WebStage::Verifying,
            Some("Sign-in cancelled."),
            60,
        ));
        assert!(failed.contains(&"Sign-in cancelled.".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_first_prompt_leads_with_the_sign_in_that_reaches_messages() {
        let (title, lines) = browser_lines(&BrowserPhase::Ready, None, 60);
        let lines = text(&lines);
        assert_eq!(title, "Connect to Discord");
        assert!(lines.iter().any(|line| line.contains("connect messages")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("account and servers only"))
        );
    }

    #[test]
    fn token_is_masked_and_error_is_shown() {
        let lines = text(&token_entry_lines("secret", Some("Failed to connect")));
        assert!(lines.iter().all(|line| !line.contains("secret")));
        assert!(lines.contains(
            &"Token  \u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2588}".to_string()
        ));
        assert!(lines.contains(&"Failed to connect".to_string()));
    }

    #[test]
    fn empty_token_shows_a_placeholder() {
        let lines = text(&token_entry_lines("", None));
        assert!(lines.contains(&"Token  paste your Discord token".to_string()));
    }

    #[test]
    fn account_screen_offers_only_the_keys_that_work_in_each_phase() {
        let keys = |phase: &BrowserPhase| {
            let (_, lines) = browser_lines(phase, None, 54);
            text(&lines).join("|")
        };
        let ready = keys(&BrowserPhase::Ready);
        #[cfg(target_os = "macos")]
        {
            assert!(ready.contains("enter connect messages"));
            assert!(ready.contains("b account and servers only"));
            assert!(ready.contains("Keychain"));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(ready.contains("enter open browser"));
            assert!(ready.contains("name and servers"));
        }
        assert!(ready.contains("t paste a token"));
        assert!(ready.contains("esc sample data   q quit"));

        let waiting = keys(&BrowserPhase::Waiting);
        assert!(waiting.contains("esc cancel   q quit"));
        assert!(!waiting.contains("open browser") && !waiting.contains("sign out"));

        for busy in [
            BrowserPhase::Restore,
            BrowserPhase::SigningOut(crate::app::BrowserAccount::new("pilot".into(), vec![])),
        ] {
            let busy = keys(&busy);
            assert!(busy.ends_with("q quit   "), "{busy}");
            assert!(!busy.contains("esc") && !busy.contains("enter"), "{busy}");
        }

        let signed_in = keys(&BrowserPhase::SignedIn(crate::app::BrowserAccount::new(
            "pilot".into(),
            vec![],
        )));
        assert!(signed_in.starts_with("Signed in as pilot|"));
        assert!(signed_in.contains("message token"));
        let next = signed_in.find("t paste a token");
        let sign_out = signed_in.find("s sign out");
        assert!(next.is_some() && next < sign_out, "{signed_in}");
        assert!(!signed_in.contains("open browser"));
    }

    /// The first prompt's title differs by platform: only macOS can open the
    /// sign-in window that reaches messages.
    #[cfg(target_os = "macos")]
    const READY_TITLE: &str = "Connect to Discord";
    #[cfg(not(target_os = "macos"))]
    const READY_TITLE: &str = "Account and server list only";

    #[test]
    fn account_screen_titles_and_errors() {
        let (title, lines) = browser_lines(&BrowserPhase::AccountReady, None, 54);
        assert_eq!(title, ACCOUNT_TITLE);
        let hints = text(&lines).join(" ");
        assert!(hints.contains("enter open browser"));
        assert!(!hints.contains("connect messages"));

        let (title, lines) = browser_lines(
            &BrowserPhase::Ready,
            Some("Could not open the browser callback on port 38547"),
            54,
        );
        assert_eq!(title, READY_TITLE);
        assert!(text(&lines).contains(&"Could not open the browser callback on port 38547".into()));

        let (title, _) = browser_lines(
            &BrowserPhase::SignedIn(crate::app::BrowserAccount::new("pilot".into(), vec![])),
            None,
            54,
        );
        assert_eq!(title, "Account and server list only");
    }
}
