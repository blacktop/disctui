use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::model::{ChannelKind, ChannelSummary};
use crate::ui::theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    channels: &[ChannelSummary],
    state: &mut ListState,
    focused: bool,
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

                if ch.unread {
                    let mut spans = vec![
                        Span::styled("  \u{25cf} ", theme::unread()), // bold cyan dot
                        Span::styled(
                            format!("# {name}"),
                            Style::new()
                                .fg(ratatui::style::Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ];
                    if ch.unread_count > 0 {
                        spans.push(Span::styled(
                            format!(" ({})", ch.unread_count),
                            theme::mention(),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                } else {
                    let label = format!("    # {name}");
                    ListItem::new(Line::from(label.dim()))
                }
            }
        })
        .collect();

    let selectable_count = channels
        .iter()
        .filter(|channel| channel.kind.is_selectable())
        .count();
    let title = match selected_channel_id.and_then(|id| channels.iter().find(|ch| ch.id == id)) {
        Some(selected) => format!(
            " Channels {} · #{} ",
            selectable_count,
            truncate_name(&selected.name, 16)
        ),
        None => format!(" Channels {selectable_count} "),
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title.bold().cyan()),
        )
        .highlight_style(theme::selected_item())
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, state);
}

use super::truncate_name;
