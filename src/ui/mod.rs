mod channels;
mod guilds;
mod input;
pub mod layout;
pub mod media;
mod messages;
mod popup;
mod status_bar;
mod summary;
pub mod theme;

use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, FocusPane, MessagePaneView};

/// Top-level render dispatch. Composes all widgets into the final frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let [body, status_area] = layout::root(frame.area());
    let [guild_area, channel_area, main_area] =
        layout::main_body(body, app.focus, app.selected_channel_id.is_some());
    // Split main area: header (1 line) + content + input
    let [header_area, rest_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(1),
        ratatui::layout::Constraint::Fill(1),
    ])
    .areas(main_area);
    let [content_area, input_area] = layout::message_pane(rest_area, 3);

    // Header breadcrumb: Guild > #channel
    render_header(frame, header_area, app);

    let guild_pane_loading_bar = app.guild_pane_loading_bar();
    let channels_pane_loading_bar = app.channels_pane_loading_bar();
    let messages_pane_loading_bar = app.messages_pane_loading_bar();

    guilds::render(
        frame,
        guild_area,
        &app.guilds,
        &mut app.guild_state,
        app.focus == FocusPane::Guilds,
        guild_pane_loading_bar.as_deref(),
        app.selected_guild_id.as_deref(),
        &mut app.avatars,
    );

    channels::render(
        frame,
        channel_area,
        &app.channels,
        &mut app.channel_state,
        app.focus == FocusPane::Channels,
        channels_pane_loading_bar.as_deref(),
        app.selected_channel_id.as_deref(),
    );

    match app.message_pane_view {
        MessagePaneView::Messages => {
            // Single render pass: render returns max_scroll, avoiding double layout build
            let max_scroll = messages::render(
                frame,
                content_area,
                &app.messages,
                if app.at_bottom {
                    u16::MAX
                } else {
                    app.message_scroll_offset
                },
                messages_pane_loading_bar.as_deref(),
                app.focus == FocusPane::Messages,
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
                app.focus == FocusPane::Messages,
            );
        }
    }

    input::render(
        frame,
        input_area,
        &app.input_text,
        app.input_mode,
        app.focus == FocusPane::Input,
    );

    status_bar::render(
        frame,
        status_area,
        app.focus,
        app.connection_state,
        app.selected_channel_name().zip(app.selected_channel_kind()),
        app.active_load_label().zip(app.active_load_progress_bar()),
        app.status_error(),
    );

    if let Some(prompt) = app.discord_token_prompt() {
        popup::render_discord_token_prompt(frame, frame.area(), prompt);
    } else if app.show_help {
        popup::render_help(frame, frame.area());
    }
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

fn render_header(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let guild = app.selected_guild_name().unwrap_or("");
    let channel = app.selected_channel_name().unwrap_or("");
    let channel_marker = app
        .selected_channel_kind()
        .map_or("#", |kind| kind.marker());
    let channel_suffix = if app.selected_channel_directly_muted() {
        format!(" {}", theme::MUTE_GLYPH)
    } else {
        String::new()
    };
    let selected_guild_label = if app.selected_guild_muted() {
        format!("{guild} {}", theme::MUTE_GLYPH)
    } else {
        guild.to_string()
    };

    let line = if guild.is_empty() && channel.is_empty() {
        Line::from(Span::styled(" disctui", theme::title()))
    } else if channel.is_empty() {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(selected_guild_label, theme::selected_item()),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(selected_guild_label, theme::dim()),
            Span::styled(" \u{203a} ", theme::dim()),
            Span::styled(
                format!("{channel_marker}{channel}{channel_suffix}"),
                theme::selected_item(),
            ),
        ])
    };

    frame.render_widget(
        Paragraph::new(line).style(ratatui::style::Style::new().bg(theme::DISCORD_BG_DARK)),
        area,
    );
}
