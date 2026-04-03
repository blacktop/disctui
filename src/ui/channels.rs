use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::model::{ChannelKind, ChannelSummary};
use crate::ui::theme;
use crate::ui::truncate_name;

#[expect(
    clippy::too_many_arguments,
    reason = "top-level pane renderer takes explicit widget state and loading context"
)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    channels: &[ChannelSummary],
    state: &mut ListState,
    focused: bool,
    loading_bar: Option<&str>,
    guild_muted: bool,
    selected_channel_id: Option<&str>,
) {
    let border_style = if focused {
        theme::focused_border()
    } else {
        theme::unfocused_border()
    };

    let items: Vec<ListItem> = channels
        .iter()
        .map(|ch| {
            if ch.kind == ChannelKind::Category {
                let label = format!(
                    "⌄ {}",
                    truncate_name(
                        &ch.name.to_uppercase(),
                        area.width.saturating_sub(4) as usize
                    )
                );
                ListItem::new(Line::from(label.dim()))
            } else {
                let max_name = area.width.saturating_sub(10) as usize;
                let name = truncate_name(&ch.name, max_name);
                let marker = ch.kind.marker();
                let mute_marker = ch
                    .muted
                    .then(|| Span::styled(format!(" {}", theme::MUTE_GLYPH), theme::muted()));

                if ch.shows_unread(guild_muted) {
                    let mut spans = vec![
                        Span::styled("  \u{25cf} ", theme::unread()), // bold cyan dot
                        Span::styled(
                            format!("{marker} {name}"),
                            Style::new()
                                .fg(ratatui::style::Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ];
                    if let Some(marker) = mute_marker.clone() {
                        spans.push(marker);
                    }
                    if ch.unread_count > 0 {
                        spans.push(Span::styled(
                            format!(" ({})", ch.unread_count),
                            theme::mention(),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                } else {
                    let mut spans =
                        vec![Span::styled(format!("    {marker} {name}"), theme::dim())];
                    if let Some(marker) = mute_marker {
                        spans.push(marker);
                    }
                    ListItem::new(Line::from(spans))
                }
            }
        })
        .collect();

    let selectable_count = channels
        .iter()
        .filter(|channel| channel.kind.is_selectable())
        .count();
    let title = title(channels, selected_channel_id, loading_bar, selectable_count);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(title, theme::title())),
        )
        .highlight_style(theme::selected_item())
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, state);
}

fn title(
    channels: &[ChannelSummary],
    selected_channel_id: Option<&str>,
    loading_bar: Option<&str>,
    selectable_count: usize,
) -> String {
    match selected_channel_id.and_then(|id| channels.iter().find(|ch| ch.id == id)) {
        Some(selected) => format!(
            " Channels {} · {}{}{}{} ",
            selectable_count,
            selected.kind.marker(),
            truncate_name(&selected.name, 16),
            if selected.muted {
                format!(" {}", theme::MUTE_GLYPH)
            } else {
                String::new()
            },
            loading_bar.map_or_else(String::new, |bar| format!(" {bar}"))
        ),
        None => format!(
            " Channels {selectable_count}{} ",
            loading_bar.map_or_else(String::new, |bar| format!(" {bar}"))
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{ChannelKind, ChannelSummary};

    #[test]
    fn title_composes_mute_and_loading_suffixes() {
        let channels = vec![ChannelSummary {
            id: "c1".into(),
            guild_id: Some("g1".into()),
            parent_id: None,
            name: "general".into(),
            kind: ChannelKind::Text,
            position: 0,
            muted: true,
            unread: false,
            unread_count: 0,
            last_message_id: None,
        }];

        let title = super::title(&channels, Some("c1"), Some("▱▰▰▱"), 1);
        assert!(title.contains("general"));
        assert!(title.contains(super::theme::MUTE_GLYPH));
        assert!(title.contains("▱▰▰▱"));
    }
}
