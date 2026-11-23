use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use unicode_width::UnicodeWidthStr;

use crate::model::{ChannelKind, ChannelSummary};
use crate::ui::theme;
use crate::ui::truncate_name;

/// Title row and focus state of the channel pane.
pub struct PaneChrome<'a> {
    pub title: &'a str,
    pub focused: bool,
    pub loading_bar: Option<&'a str>,
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    channels: &[ChannelSummary],
    state: &mut ListState,
    chrome: &PaneChrome,
) {
    // The right-hand hairline takes one column.
    let row_width = usize::from(area.width.saturating_sub(1));
    let cursor = state.selected();

    let items: Vec<ListItem> = channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            if channel.kind == ChannelKind::Category {
                category_item(channel, index, row_width)
            } else {
                ListItem::new(channel_line(channel, row_width, cursor == Some(index)))
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::new()
                .borders(Borders::RIGHT)
                .border_style(theme::divider())
                .title(title_line(chrome, row_width)),
        )
        .highlight_style(theme::selected_row(chrome.focused));

    frame.render_stateful_widget(list, area, state);
}

fn title_line(chrome: &PaneChrome, row_width: usize) -> Line<'static> {
    let bar_width = chrome.loading_bar.map_or(0, |bar| bar.width() + 1);
    let name = truncate_name(
        &chrome.title.to_uppercase(),
        row_width.saturating_sub(bar_width + 2),
    );
    let mut spans = vec![Span::styled(
        format!(" {name}"),
        theme::pane_title(chrome.focused),
    )];
    if let Some(bar) = chrome.loading_bar {
        spans.push(Span::styled(format!(" {bar}"), theme::count()));
    }
    Line::from(spans)
}

/// Categories get a blank row above them, except at the top of the list.
fn category_item(channel: &ChannelSummary, index: usize, row_width: usize) -> ListItem<'static> {
    let name = truncate_name(&channel.name, row_width.saturating_sub(2));
    let label = Line::from(Span::styled(format!(" {name}"), theme::dim()));
    if index == 0 {
        ListItem::new(label)
    } else {
        ListItem::new(vec![Line::raw(""), label])
    }
}

/// One channel row: unread marker, name, then the mention count flush right.
fn channel_line(channel: &ChannelSummary, row_width: usize, is_cursor: bool) -> Line<'static> {
    let unread = channel.shows_unread_in_channel_list();
    let count = if unread && channel.unread_count > 0 {
        channel.unread_count.to_string()
    } else {
        String::new()
    };
    let mute = if channel.muted {
        format!(" {}", theme::MUTE_GLYPH)
    } else {
        String::new()
    };

    // Layout: " ● # name ⊘" + padding + "count "
    let fixed = 3 + 2 + mute.width() + count.width() + 1;
    let name = truncate_name(&channel.name, row_width.saturating_sub(fixed));
    let label = format!("{} {name}", channel.kind.marker());
    let used = 3 + label.width() + mute.width() + count.width() + 1;
    let padding = " ".repeat(row_width.saturating_sub(used));

    let (marker, label_style) = if unread {
        (
            Span::styled(format!(" {} ", theme::UNREAD_GLYPH), theme::unread()),
            theme::unread_name(),
        )
    } else if is_cursor {
        (Span::raw("   "), ratatui::style::Style::new())
    } else {
        (Span::raw("   "), theme::dim())
    };

    Line::from(vec![
        marker,
        Span::styled(label, label_style),
        Span::styled(mute, theme::muted()),
        Span::raw(padding),
        Span::styled(count, theme::count()),
        Span::raw(" "),
    ])
}

#[cfg(test)]
mod tests {
    use crate::ui::channels::*;

    fn channel(name: &str, unread: bool, mentions: u32, muted: bool) -> ChannelSummary {
        ChannelSummary {
            id: "c1".into(),
            guild_id: Some("g1".into()),
            parent_id: None,
            name: name.into(),
            kind: ChannelKind::Text,
            position: 0,
            muted,
            unread,
            unread_count: mentions,
            last_message_id: None,
        }
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn unread_row_has_marker_and_right_aligned_count() {
        let line = channel_line(&channel("general", true, 2, false), 20, false);
        assert_eq!(text(&line), " \u{25cf} # general      2 ");
        assert_eq!(line.width(), 20);
    }

    #[test]
    fn read_row_has_no_marker_or_count() {
        let line = channel_line(&channel("general", false, 0, false), 20, false);
        assert_eq!(text(&line).trim_end(), "   # general");
    }

    #[test]
    fn muted_unread_channel_shows_mute_glyph_without_unread_marker() {
        let line = channel_line(&channel("general", true, 3, true), 20, false);
        let rendered = text(&line);
        assert!(rendered.contains(theme::MUTE_GLYPH));
        assert!(!rendered.contains(theme::UNREAD_GLYPH));
        assert!(!rendered.contains('3'));
    }

    #[test]
    fn long_names_are_truncated_to_fit_the_row() {
        let line = channel_line(
            &channel("a-very-long-channel-name", true, 12, false),
            16,
            false,
        );
        assert!(line.width() <= 16);
        assert!(text(&line).contains('\u{2026}'));
        assert!(text(&line).trim_end().ends_with("12"));
    }

    #[test]
    fn zero_width_row_does_not_panic() {
        let line = channel_line(&channel("general", true, 2, false), 0, false);
        assert!(text(&line).contains('#'));
    }

    #[test]
    fn title_uppercases_and_appends_loading_bar() {
        let chrome = PaneChrome {
            title: "Rust Lang",
            focused: true,
            loading_bar: Some("▱▰▰▱"),
        };
        assert_eq!(text(&title_line(&chrome, 30)), " RUST LANG ▱▰▰▱");
    }

    #[test]
    fn first_category_has_no_spacer_row() {
        let mut category = channel("The Basics", false, 0, false);
        category.kind = ChannelKind::Category;
        assert_eq!(category_item(&category, 0, 20).height(), 1);
        assert_eq!(category_item(&category, 3, 20).height(), 2);
    }
}
