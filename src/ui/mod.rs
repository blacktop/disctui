mod channels;
mod digest;
mod guilds;
mod input;
pub mod layout;
pub mod media;
mod messages;
mod popup;
mod status_bar;
mod summary;
pub mod theme;
mod top_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, FocusPane, MessagePaneView};

/// Divider row plus two lines of text.
const COMPOSER_HEIGHT: u16 = 3;

/// Top-level render dispatch. Composes all widgets into the final frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let [top_area, body, footer_area] = layout::root(frame.area());

    top_bar::render(frame, top_area, &top_bar::TopBar::from_app(app));
    let popup_area = if let Some((account, avatars)) = app.account_rail() {
        // Reserve space for account controls next to the list, even on narrow terminals.
        let rail_width = (body.width / 3).min(34);
        let [rail, account_area] =
            Layout::horizontal([Constraint::Length(rail_width), Constraint::Fill(1)]).areas(body);
        guilds::render(
            frame,
            rail,
            &account.guilds,
            &mut account.selection,
            true,
            None,
            None,
            avatars,
            None,
        );
        account_area
    } else {
        if !(app.has_sign_in() && app.guilds.is_empty()) {
            render_panes(frame, body, app);
        }
        frame.area()
    };
    status_bar::render(frame, footer_area, &status_bar::Footer::from_app(app));

    if let Some(sign_in) = app.sign_in() {
        popup::render_sign_in(frame, popup_area, sign_in);
    } else if app.show_help {
        popup::render_help(frame, frame.area());
    }
}

/// Servers, channels, and the message pane with its composer.
fn render_panes(frame: &mut Frame, body: Rect, app: &mut App) {
    let [guild_area, channel_area, main_area] = if app.digest.visible {
        Layout::horizontal([
            Constraint::Length(9),
            Constraint::Length(0),
            Constraint::Fill(1),
        ])
        .areas(body)
    } else {
        layout::main_body(body, app.focus)
    };

    let guild_pane_loading_bar = app.guild_pane_loading_bar();
    guilds::render(
        frame,
        guild_area,
        &app.guilds,
        &mut app.guild_state,
        app.focus == FocusPane::Guilds && !app.digest.visible,
        guild_pane_loading_bar.as_deref(),
        app.selected_guild_id
            .as_deref()
            .filter(|_| !app.digest.visible),
        &mut app.avatars,
        Some(&app.digest),
    );

    if app.digest.visible {
        digest::render(frame, main_area, &mut app.digest);
        return;
    }

    let [content_area, input_area] = layout::message_pane(main_area, COMPOSER_HEIGHT);
    let channels_pane_loading_bar = app.channels_pane_loading_bar();
    let unread_divider_message_id = app.unread_divider_message_id().map(str::to_string);
    let channels_title = app.selected_guild_name().unwrap_or("Channels").to_string();

    channels::render(
        frame,
        channel_area,
        &app.channels,
        &mut app.channel_state,
        &channels::PaneChrome {
            title: &channels_title,
            focused: app.focus == FocusPane::Channels,
            loading_bar: channels_pane_loading_bar.as_deref(),
        },
    );

    if main_area.is_empty() {
        return;
    }

    match app.message_pane_view {
        MessagePaneView::Messages
            if app.messages.is_empty() && app.selected_history_status().is_some() =>
        {
            frame.render_widget(
                Paragraph::new(app.selected_history_status().unwrap_or_default())
                    .style(theme::dim())
                    .wrap(Wrap { trim: true }),
                content_area.inner(ratatui::layout::Margin::new(1, 1)),
            );
        }
        MessagePaneView::Messages => {
            // Single render pass: render returns max_scroll, avoiding double layout build
            let max_scroll = messages::render(
                frame,
                content_area,
                &app.messages,
                unread_divider_message_id.as_deref(),
                if app.at_bottom {
                    u16::MAX
                } else {
                    app.message_scroll_offset
                },
                &mut app.avatars,
            );
            app.max_scroll = max_scroll;
            if app.at_bottom {
                app.message_scroll_offset = max_scroll;
            }
            app.message_scroll_offset = app.message_scroll_offset.min(max_scroll);
        }
        MessagePaneView::Summary => {
            summary::render(
                frame,
                content_area,
                app.summary_state.last_digest.as_ref(),
                app.summary_state.selected_todo,
                app.summary_state.in_flight,
            );
        }
    }

    input::render(
        frame,
        input_area,
        &app.input_text,
        app.input_mode,
        app.selected_channel_name().zip(app.selected_channel_kind()),
    );
}

/// Unicode-width-aware name truncation for sidebar display.
pub(super) fn truncate_name(name: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(name) <= max_width {
        name.to_string()
    } else {
        let mut out = String::new();
        let budget = max_width.saturating_sub(1);
        let mut used = 0;
        for ch in name.chars() {
            let width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + width > budget {
                break;
            }
            out.push(ch);
            used += width;
        }
        if max_width == 1 && out.is_empty() {
            "\u{2026}".to_string()
        } else {
            out.push('\u{2026}');
            out
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::action::Action;
    #[cfg(feature = "experimental-discord")]
    use crate::app::BrowserPhase;
    use crate::digest::HistorySource;
    use crate::effect::Effect;
    use crate::transport::mock;
    use crate::ui::*;

    /// The app after opening Rust Lang > #general from the sample data.
    fn app_reading_general() -> App {
        let mut app = App::new();
        app.update(Action::GuildsLoaded(mock::guilds()));
        for guild_id in ["g1", "g3"] {
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild_id.into()),
                channels: mock::channels(guild_id),
            });
        }
        app.update(Action::OpenChannel("c1".into()));
        app.update(Action::HistoryLoaded {
            channel_id: "c1".into(),
            messages: mock::messages("c1"),
            has_more: false,
        });
        app
    }

    fn render_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn reading_view_has_breadcrumb_panes_composer_and_footer() {
        let mut app = app_reading_general();
        let rows = render_rows(&mut app, 120, 30);
        dump_frame("reading", &rows);

        assert!(rows[0].starts_with(" disctui  Rust Lang \u{203a} # general"));
        assert!(rows[0].ends_with("13 loaded"));
        assert!(rows.iter().any(|row| row.contains("RUST LANG")));
        assert!(rows.iter().any(|row| row.contains("The Basics")));
        assert!(
            rows.iter()
                .any(|row| row.contains("@ferris thoughts on the new edition?"))
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("\u{203a} Message #general"))
        );
        let footer = rows.last().unwrap();
        assert!(footer.starts_with(" NORMAL  j/k scroll"));
        assert!(footer.ends_with("sample data"));
    }

    #[test]
    fn panes_have_no_box_corners() {
        let mut app = app_reading_general();
        let rows = render_rows(&mut app, 120, 30);
        let corners = ['\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}'];
        assert!(
            rows.iter().all(|row| !row.contains(corners)),
            "panes should be separated by hairlines, not boxes"
        );
    }

    #[test]
    fn help_popup_draws_over_the_panes() {
        let mut app = app_reading_general();
        app.update(Action::ToggleHelp);
        let rows = render_rows(&mut app, 120, 34);
        dump_frame("help", &rows);
        assert!(rows.iter().any(|row| row.contains("\u{256d} Keys ")));
        assert!(
            rows.iter()
                .any(|row| row.contains("Summarize unread messages"))
        );
    }

    #[test]
    fn narrow_and_tiny_terminals_render_without_panicking() {
        let mut app = app_reading_general();
        for (width, height) in [(60, 16), (30, 8), (12, 3), (1, 1)] {
            let rows = render_rows(&mut app, width, height);
            assert_eq!(rows.len(), usize::from(height));
        }
    }

    #[test]
    fn sign_in_shows_the_first_screen_on_a_standard_terminal() {
        let mut app = App::new();
        app.show_sign_in();
        let rows = render_rows(&mut app, 80, 24);
        dump_frame("sign-in", &rows);

        assert!(rows[0].starts_with(" disctui  not signed in"));
        assert!(
            rows.iter()
                .all(|row| !row.contains("SERVERS") && !row.contains("No messages yet")),
            "empty panes stay hidden behind the first sign-in"
        );
        for text in ["t paste a token", "sample data", "q quit"] {
            assert!(rows.iter().any(|row| row.contains(text)), "missing {text}");
        }
        let footer = rows.last().unwrap();
        assert!(footer.starts_with(" SIGN IN "));
        assert!(footer.ends_with("disconnected"));

        for (width, height) in [(30, 8), (12, 3), (1, 1)] {
            assert_eq!(
                render_rows(&mut app, width, height).len(),
                usize::from(height)
            );
        }
    }

    #[test]
    fn sign_in_over_sample_data_keeps_the_panes() {
        let mut app = app_reading_general();
        app.show_sign_in();
        let rows = render_rows(&mut app, 120, 30);
        assert!(rows.iter().any(|row| row.contains("t paste a token")));
        assert!(rows.iter().any(|row| row.contains("RUST LANG")));
    }

    #[test]
    fn token_entry_replaces_the_first_screen() {
        let mut app = App::new();
        app.show_sign_in();
        app.show_token_entry();
        app.push_token_char('s');
        let rows = render_rows(&mut app, 80, 24);
        assert!(rows.iter().all(|row| !row.contains("sample data")));
        assert!(rows.iter().any(|row| row.contains("Token  \u{2022}")));
        assert!(rows.iter().any(|row| row.contains("esc back")));
    }

    fn dump_frame(name: &str, rows: &[String]) {
        if std::env::var_os("DISCTUI_PRINT_FRAMES").is_some() {
            std::fs::write(
                std::env::temp_dir().join(format!("disctui-frame-{name}.txt")),
                rows.join("\n"),
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn digest_view_lists_what_needs_you_and_the_top_channels() {
        let mut app = app_reading_general();
        app.update(Action::SetFocus(FocusPane::Guilds));
        app.update(Action::JumpTop);
        let effects = app.update(Action::OpenSelected);
        let Some(Effect::GenerateDigest { job, request }) = effects.into_iter().next() else {
            unreachable!("opening the digest entry asks for a digest");
        };

        let waiting = render_rows(&mut app, 120, 30);
        assert!(
            waiting
                .iter()
                .any(|row| row.contains("Reading channel history"))
        );

        let report = request
            .generate(HistorySource::Mock, None, |_, _, _| {})
            .await;
        app.update(Action::DigestReady { job, report });
        let rows = render_rows(&mut app, 120, 30);
        dump_frame("digest", &rows);

        for text in ["NEEDS YOU", "TOP CHANNELS", "Rust Lang", "Tokio"] {
            assert!(rows.iter().any(|row| row.contains(text)), "missing {text}");
        }
        // The open channel counts as read, so its mention is not repeated here.
        assert!(rows.iter().any(|row| row.contains("NEEDS YOU  2")));
        assert!(
            rows.iter()
                .any(|row| row.contains("Unanswered direct message"))
        );
        let footer = rows.last().unwrap();
        assert!(footer.starts_with(" DIGEST  j/k move"));
    }

    #[cfg(feature = "experimental-discord")]
    #[test]
    fn account_screen_follows_the_browser_sign_in() {
        let mut app = App::new();
        app.set_browser_phase(BrowserPhase::Ready);
        let rows = render_rows(&mut app, 80, 24);
        dump_frame("account-ready", &rows);
        #[cfg(target_os = "macos")]
        {
            assert!(rows.iter().any(|row| row.contains("Connect to Discord")));
            assert!(
                rows.iter()
                    .any(|row| row.contains("enter connect messages"))
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(
                rows.iter()
                    .any(|row| row.contains("Account and server list only"))
            );
            assert!(rows.iter().any(|row| row.contains("enter open browser")));
        }

        app.set_browser_phase(BrowserPhase::Waiting);
        let rows = render_rows(&mut app, 80, 24);
        dump_frame("account-waiting", &rows);
        assert!(rows.iter().any(|row| row.contains("esc cancel   q quit")));

        let guild = crate::transport::discord::guild_to_summary("1", "Test Server", Some("icon"));
        let second = crate::transport::discord::guild_to_summary("2", "Beta", None);
        let effects = app.show_browser_account("pilot".into(), vec![guild, second]);
        assert!(
            matches!(effects.as_slice(), [crate::effect::Effect::FetchAvatar { url }] if url == "https://cdn.discordapp.com/icons/1/icon.png?size=64")
        );
        app.set_sign_in_error("Signed out locally.".into());
        let rows = render_rows(&mut app, 80, 24);
        dump_frame("account-signed-in", &rows);
        assert!(rows.iter().any(|row| row.contains("SERVERS 2")));
        assert!(rows.iter().any(|row| row.contains("Test Server")));
        assert!(rows.iter().any(|row| row.contains("Signed in as pilot")));
        assert!(rows.iter().any(|row| row.contains("s sign out")));
        assert!(rows.last().unwrap().starts_with(" ACCOUNT "));
        app.update(crate::action::Action::MoveDown);
        assert!(app.update(crate::action::Action::OpenSelected).is_empty());
        let rows = render_rows(&mut app, 120, 24);
        assert!(
            rows.iter()
                .any(|row| row.contains("message token is required to open channels in Beta"))
        );
        assert!(
            app.guilds.is_empty(),
            "account navigation must not populate the message transport"
        );
    }

    #[test]
    fn empty_app_explains_itself() {
        let mut app = App::new();
        let rows = render_rows(&mut app, 100, 20);
        assert!(rows[0].contains("no server selected"));
        assert!(rows.iter().any(|row| row.contains("No servers loaded")));
    }
}
