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

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::action::{Action, PaneRow};
use crate::app::{App, FocusPane, MessagePaneView};

/// Divider row plus two lines of text.
const COMPOSER_HEIGHT: u16 = 3;

/// Hit areas from the last drawn frame, including widget scroll offsets.
#[derive(Default)]
pub struct MouseTargets {
    guilds: Rect,
    max_guild_offset: usize,
    channels: Rect,
    channel_viewport: (u16, usize),
    main: Rect,
    rows: Vec<(Rect, PaneRow)>,
}

pub fn mouse_action(app: &App, event: MouseEvent) -> Option<Action> {
    let point = Position::new(event.column, event.row);
    let targets = &app.mouse_targets;
    let up = match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            return targets
                .rows
                .iter()
                .find(|(area, _)| area.contains(point))
                .map(|(_, row)| Action::ClickRow(*row));
        }
        MouseEventKind::ScrollUp => true,
        MouseEventKind::ScrollDown => false,
        _ => return None,
    };

    if targets.guilds.contains(point) {
        wheel_offset(
            app.guild_state.offset(),
            if up { -1 } else { 1 },
            targets.max_guild_offset,
        )
        .map(Action::ScrollGuilds)
    } else if targets.channels.contains(point) {
        let (height, max_offset) = targets.channel_viewport;
        if height == 0 {
            return None;
        }
        let offset = wheel_offset(
            app.channel_state.offset(),
            if up { -3 } else { 3 },
            max_offset,
        )?;
        Some(Action::ScrollChannels {
            offset,
            selection: channels::wheel_selection(
                &app.channels,
                offset,
                height,
                app.channel_state.selected(),
            ),
        })
    } else if targets.main.contains(point) {
        Some(match (app.digest.visible, app.message_pane_view, up) {
            (false, MessagePaneView::Summary, true) => Action::SummarySelectPrevTodo,
            (false, MessagePaneView::Summary, false) => Action::SummarySelectNextTodo,
            (_, _, true) => Action::ScrollUp(3),
            (_, _, false) => Action::ScrollDown(3),
        })
    } else {
        None
    }
}

fn wheel_offset(offset: usize, delta: isize, max_offset: usize) -> Option<usize> {
    let next = offset
        .saturating_add_signed(delta)
        .min(max_offset.max(offset));
    (next != offset).then_some(next)
}

/// Top-level render dispatch. Composes all widgets into the final frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let [top_area, body, footer_area] = layout::root(frame.area());
    app.mouse_targets.rows.clear();
    app.mouse_targets = MouseTargets {
        rows: std::mem::take(&mut app.mouse_targets.rows),
        ..MouseTargets::default()
    };

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
    } else if let Some(picker) = &mut app.link_picker {
        popup::render_link_picker(frame, frame.area(), picker);
    }
}

/// Servers, channels, and the message pane with its composer.
fn render_panes(frame: &mut Frame, body: Rect, app: &mut App) {
    app.message_cursor.visible = None;
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
    app.mouse_targets.guilds = guild_area;
    app.mouse_targets.max_guild_offset = guilds::render(
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
        Some(&mut app.mouse_targets.rows),
    );

    if app.digest.visible {
        app.mouse_targets.main = main_area;
        digest::render(frame, main_area, &mut app.digest);
        return;
    }

    let [content_area, input_area] = layout::message_pane(main_area, COMPOSER_HEIGHT);
    let channels_pane_loading_bar = app.channels_pane_loading_bar();
    let unread_divider_message_id = app.unread_divider_message_id().map(str::to_string);
    let channels_title = app.selected_guild_name().unwrap_or("Channels").to_string();

    app.mouse_targets.channels = channel_area;
    app.mouse_targets.channel_viewport = channels::render(
        frame,
        channel_area,
        &app.channels,
        &mut app.channel_state,
        &channels::PaneChrome {
            title: &channels_title,
            focused: app.focus == FocusPane::Channels,
            loading_bar: channels_pane_loading_bar.as_deref(),
        },
        &mut app.mouse_targets.rows,
    );

    if main_area.is_empty() {
        return;
    }

    app.mouse_targets.main = content_area;
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
            let (max_scroll, actual_offset) = messages::render(
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
                &mut app.message_cursor,
                &mut app.mouse_targets.rows,
            );
            app.max_scroll = max_scroll;
            app.message_scroll_offset = actual_offset;
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

    fn send_mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) -> Vec<Effect> {
        let event = crossterm::event::Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        crate::map_runtime_event(&event, app).map_or_else(Vec::new, |action| app.update(action))
    }

    fn row_containing(rows: &[String], text: &str) -> u16 {
        u16::try_from(rows.iter().position(|row| row.contains(text)).unwrap()).unwrap()
    }

    #[test]
    fn mouse_opens_servers_and_channels_and_ignores_category_rows() {
        let mut app = app_reading_general();
        let click = MouseEventKind::Down(MouseButton::Left);
        render_rows(&mut app, 112, 24);
        // Third server in the collapsed rail, below the title and Digest.
        send_mouse(&mut app, click, 2, 14);
        assert_eq!(app.selected_guild_id.as_deref(), Some("g3"));
        assert_eq!(app.focus, FocusPane::Channels);
        render_rows(&mut app, 112, 24);
        send_mouse(&mut app, click, 2, 14);
        assert_eq!(app.selected_guild_id.as_deref(), Some("g3"));

        let rows = render_rows(&mut app, 96, 24);
        let category = row_containing(&rows, "Async");
        assert!(send_mouse(&mut app, click, 12, category).is_empty());
        assert!(app.selected_channel_id.is_none());
        let help = row_containing(&rows, "# help");

        app.update(Action::ToggleHelp);
        render_rows(&mut app, 96, 24);
        assert!(send_mouse(&mut app, click, 12, help).is_empty());
        assert!(app.selected_channel_id.is_none());
        app.update(Action::ToggleHelp);
        render_rows(&mut app, 96, 24);
        assert!(matches!(
            send_mouse(&mut app, click, 12, help).as_slice(),
            [Effect::LoadHistory { channel_id }] if channel_id == "c21"
        ));
        assert_eq!(app.focus, FocusPane::Messages);
    }

    #[test]
    fn mouse_wheel_does_not_skip_channel_categories_to_follow_the_cursor() {
        let mut app = app_reading_general();
        let fixture = mock::channels("g1");
        let category = fixture
            .iter()
            .find(|row| !row.kind.is_selectable())
            .unwrap();
        let channel = fixture.iter().find(|row| row.kind.is_selectable()).unwrap();
        app.channels = (0..8)
            .flat_map(|index| {
                let mut category = category.clone();
                category.id = format!("section{index}");
                let mut channel = channel.clone();
                channel.id = format!("channel{index}");
                channel.parent_id = Some(category.id.clone());
                [category, channel]
            })
            .collect();
        app.channel_state.select(Some(3));
        render_rows(&mut app, 110, 9);
        assert_eq!(app.channel_state.offset(), 0);

        send_mouse(&mut app, MouseEventKind::ScrollDown, 10, 4);
        render_rows(&mut app, 110, 9);
        assert_eq!(app.channel_state.offset(), 3);
    }

    #[test]
    fn wheel_scrolls_the_hovered_pane_and_clicked_wrapped_text_copies_the_full_post() {
        use crate::message_actions::DesktopAction;
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        let mut app = app_reading_general();
        let mut messages = mock::messages("c1");
        messages.truncate(2);
        let post = format!("first line\n{}\nlast line", "words ".repeat(100));
        messages.first_mut().unwrap().content.clone_from(&post);
        app.update(Action::HistoryLoaded {
            channel_id: "c1".into(),
            messages,
            has_more: false,
        });
        render_rows(&mut app, 110, 9);
        let offset = app.message_scroll_offset;
        let guild_offset = app.guild_state.offset();
        let channel_offset = app.channel_state.offset();
        send_mouse(&mut app, MouseEventKind::ScrollDown, 2, 4);
        assert!(app.guild_state.offset() > guild_offset);
        assert_eq!(app.selected_guild_id.as_deref(), Some("g1"));
        send_mouse(&mut app, MouseEventKind::ScrollDown, 10, 4);
        assert!(app.channel_state.offset() > channel_offset);
        assert_eq!(app.message_scroll_offset, offset);
        assert_eq!(app.selected_channel_id.as_deref(), Some("c1"));

        render_rows(&mut app, 110, 9);
        send_mouse(&mut app, MouseEventKind::ScrollUp, 46, 3);
        assert!(app.message_scroll_offset < offset);
        let rows = render_rows(&mut app, 110, 9);
        let wrapped = row_containing(&rows, "words words");
        let before_click = app.message_scroll_offset;
        send_mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            46,
            wrapped,
        );
        render_rows(&mut app, 110, 9);
        assert_eq!(app.message_scroll_offset, before_click);
        let copy = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let action = crate::map_runtime_event(&copy, &mut app).unwrap();
        assert!(
            matches!(app.update(action).as_slice(), [Effect::Desktop(DesktopAction::Copy(text))] if text == &post)
        );
    }

    #[test]
    fn message_actions_select_a_long_post_and_copy_its_entire_source() {
        use crate::app::InputMode;
        use crate::message_actions::DesktopAction;
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        let mut app = app_reading_general();
        let mut messages = mock::messages("c1");
        messages.truncate(2);
        let post = format!(
            "Start of the long post\n{}\nhttps://example.com/one [second](https://example.com/two)",
            "additional detail ".repeat(600)
        );
        let first = messages.first_mut().unwrap();
        first.content.clone_from(&post);
        let id = first.id.clone();
        app.update(Action::HistoryLoaded {
            channel_id: "c1".into(),
            messages,
            has_more: false,
        });
        let _ = render_rows(&mut app, 100, 24);
        let action = |ch| {
            crate::event::map_terminal_event(
                &Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                InputMode::Normal,
                FocusPane::Messages,
            )
            .unwrap()
        };

        assert!(app.update(action('K')).is_empty());
        let rows = render_rows(&mut app, 100, 24);
        assert_eq!(app.message_cursor.selected.as_deref(), Some(id.as_str()));
        assert!(
            rows.iter()
                .any(|row| row.contains("Start of the long post") && row.contains('▌'))
        );

        let effects = app.update(action('c'));
        assert!(
            matches!(effects.as_slice(), [Effect::Desktop(DesktopAction::Copy(text))] if text == &post)
        );
        app.update(Action::DesktopFinished(Ok("Copied message")));
        assert!(
            render_rows(&mut app, 100, 24)
                .last()
                .unwrap()
                .contains("Copied message")
        );

        assert!(app.update(action('o')).is_empty());
        assert!(
            render_rows(&mut app, 100, 24)
                .iter()
                .any(|row| row.contains("Open link"))
        );
        app.update(Action::ShowMessages);
        assert!(app.link_picker.is_none());
        app.update(action('o'));
        let _ = render_rows(&mut app, 100, 3);
        assert!(app.update(Action::OpenSelected).is_empty());
        assert!(app.link_picker.is_some());
        app.update(Action::MoveDown);
        let _ = render_rows(&mut app, 100, 24);
        assert!(matches!(app.update(Action::OpenSelected).as_slice(),
            [Effect::Desktop(DesktopAction::Open(url))] if url.as_str() == "https://example.com/two"));
    }

    #[test]
    fn scrolling_keeps_the_highlighted_post_until_it_leaves_the_viewport() {
        let mut app = app_reading_general();
        let mut messages = mock::messages("c1");
        messages.truncate(2);
        let first = messages.first_mut().unwrap();
        first.content = "Long post\n".repeat(90);
        let id = first.id.clone();
        app.update(Action::HistoryLoaded {
            channel_id: "c1".into(),
            messages,
            has_more: false,
        });
        let _ = render_rows(&mut app, 100, 24);
        app.update(Action::SelectMessage { older: true });
        let _ = render_rows(&mut app, 100, 24);

        app.update(Action::ScrollDown(1));
        let _ = render_rows(&mut app, 100, 24);
        assert_eq!(app.message_cursor.selected.as_deref(), Some(id.as_str()));

        app.update(Action::ScrollDown(u16::MAX));
        // A short viewport can put the selected post entirely above the screen.
        let _ = render_rows(&mut app, 100, 8);
        app.update(Action::ScrollDown(u16::MAX));
        let _ = render_rows(&mut app, 100, 8);
        assert!(app.message_cursor.selected.is_none());
        assert_ne!(app.message_cursor.visible.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn clipboard_success_preserves_a_connection_error() {
        let mut app = App::new();
        app.update(Action::TransportDisconnected("Network unavailable".into()));
        app.update(Action::DesktopFinished(Ok("Copied message")));
        assert_eq!(
            app.status_error(),
            Some("Network unavailable. Press Shift+C to reconnect.")
        );
    }

    #[cfg(feature = "experimental-discord")]
    #[test]
    fn rich_bot_posts_have_a_body_in_channel_history() {
        let author = serde_json::json!({"id": "20", "username": "release-bot", "bot": true});
        let history = serde_json::json!([
            {"id": "3", "author": author, "content": "", "timestamp": "2026-11-25T07:58:00Z",
                "embeds": [{"type": "image", "image": {"url": "https://example.com/preview.png"}}]},
            {"id": "2", "author": author, "content": "", "timestamp": "2026-11-25T07:57:00Z",
                "components": [{"type": 17, "components": [{"type": 9, "components": [
                    {"type": 10, "content": "Download the new build"}
                ]}]}]},
            {"id": "1", "author": author, "content": "", "timestamp": "2026-11-25T03:47:00Z",
                "embeds": [{"type": "rich", "title": "Rex Preview", "description": "A new build is ready",
                    "fields": [{"name": "Version", "value": "0.1.0-preview"}]}]}
        ]);
        let messages = crate::transport::discord::history_rows_from_value("c1", history).unwrap();
        let mut app = app_reading_general();
        app.update(Action::HistoryLoaded {
            channel_id: "c1".into(),
            messages,
            has_more: false,
        });
        let screen = render_rows(&mut app, 120, 30).join("\n");
        for text in [
            "release-bot",
            "Rex Preview",
            "A new build is ready",
            "Version",
            "0.1.0-preview",
            "Download the new build",
            "[Rich content]",
        ] {
            assert!(screen.contains(text), "missing {text} in frame:\n{screen}");
        }
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
