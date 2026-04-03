use crate::model::GuildSummary;
use crate::ui::media::{AvatarStore, AvatarTone, badge_from_name};
use crate::ui::theme;
use crate::ui::truncate_name;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

const EXPANDED_ROW_HEIGHT: u16 = 3;
const COLLAPSED_ROW_HEIGHT: u16 = 5;
const AVATAR_X_OFFSET: u16 = 1;
const EXPANDED_AVATAR_WIDTH: u16 = 5;
const EXPANDED_TEXT_GAP: u16 = 1;
const COLLAPSED_AVATAR_VERTICAL_MARGIN: u16 = 1;

#[expect(
    clippy::too_many_arguments,
    reason = "top-level pane renderer takes explicit widget state and loading context"
)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    state: &mut ListState,
    focused: bool,
    loading_bar: Option<&str>,
    selected_guild_id: Option<&str>,
    avatars: &mut AvatarStore,
) {
    let border_style = if focused {
        theme::focused_border()
    } else {
        theme::unfocused_border()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            title(guilds, selected_guild_id, loading_bar),
            theme::title(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let collapsed = !focused;
    let show_preview = !collapsed && selected_guild_id.is_some() && inner.height >= 8;
    let (preview_area, list_area) = if show_preview {
        let [preview, list] =
            Layout::vertical([Constraint::Length(4), Constraint::Fill(1)]).areas(inner);
        (Some(preview), list)
    } else {
        (None, inner)
    };

    if let Some(preview_area) = preview_area {
        render_selected_guild_preview(frame, preview_area, guilds, selected_guild_id, avatars);
    }

    let items: Vec<ListItem> = guilds
        .iter()
        .map(|guild| list_item(guild, list_area.width, collapsed))
        .collect();

    let list = List::new(items)
        .highlight_style(theme::selected_panel_row())
        .highlight_symbol(if collapsed { "" } else { "▌" });
    frame.render_stateful_widget(list, list_area, state);

    render_row_backgrounds(
        frame,
        list_area,
        guilds,
        state,
        selected_guild_id,
        collapsed,
    );
    render_guild_avatars(
        frame,
        list_area,
        guilds,
        state,
        selected_guild_id,
        avatars,
        collapsed,
    );
}

fn list_item(guild: &GuildSummary, width: u16, collapsed: bool) -> ListItem<'static> {
    if collapsed {
        return ListItem::new(blank_lines(COLLAPSED_ROW_HEIGHT));
    }

    let text_offset = expanded_text_x_offset(width);
    let text_width = usize::from(width.saturating_sub(text_offset));
    let name = truncate_name(&guild.name, text_width.saturating_sub(6));
    let unread_suffix = if guild.unread_count > 0 {
        format!(" {}", guild.unread_count)
    } else {
        String::new()
    };
    let name_style = if guild.unread {
        Style::new()
            .fg(ratatui::style::Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };

    let mut primary = vec![Span::styled(
        format!("{:width$}{name}", "", width = usize::from(text_offset)),
        name_style,
    )];
    if guild.muted {
        primary.push(Span::styled(
            format!(" {}", theme::MUTE_GLYPH),
            theme::muted(),
        ));
    }
    if !unread_suffix.is_empty() {
        primary.push(Span::styled(unread_suffix, theme::mention()));
    }

    ListItem::new(vec![Line::from(""), Line::from(primary), Line::from("")])
}

fn render_row_backgrounds(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    state: &ListState,
    selected_guild_id: Option<&str>,
    collapsed: bool,
) {
    for (row_y, guild) in visible_guild_rows(guilds, state, area, collapsed) {
        if selected_guild_id != Some(guild.id.as_str()) {
            continue;
        }
        let target_rect = if collapsed {
            collapsed_avatar_cell_rect(area, row_y)
        } else {
            Rect {
                x: area.x,
                y: row_y,
                width: area.width,
                height: EXPANDED_ROW_HEIGHT.min(area.y + area.height - row_y),
            }
        };
        frame.render_widget(
            Paragraph::new(String::new()).style(if collapsed {
                theme::selected_avatar_only_row()
            } else {
                theme::selected_panel_row()
            }),
            target_rect,
        );
    }
}

fn render_guild_avatars(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    state: &ListState,
    selected_guild_id: Option<&str>,
    avatars: &mut AvatarStore,
    collapsed: bool,
) {
    for (row_y, guild) in visible_guild_rows(guilds, state, area, collapsed) {
        let is_selected = selected_guild_id == Some(guild.id.as_str());
        let avatar_rect = if collapsed {
            collapsed_avatar_rect(area, row_y)
        } else {
            expanded_avatar_rect(area, row_y)
        };
        let tone = if collapsed && !is_selected {
            AvatarTone::Muted
        } else {
            AvatarTone::FullColor
        };
        avatars.render_avatar(
            frame,
            avatar_rect,
            guild.avatar_url.as_deref(),
            &badge_from_name(&guild.name),
            ratatui::style::Color::Rgb(88, 101, 242),
            tone,
        );
    }
}

fn collapsed_avatar_cell_rect(area: Rect, row_y: u16) -> Rect {
    Rect {
        x: area.x,
        y: row_y,
        width: area.width.max(1),
        height: COLLAPSED_ROW_HEIGHT.min(area.y + area.height - row_y),
    }
}

fn collapsed_avatar_rect(area: Rect, row_y: u16) -> Rect {
    collapsed_avatar_cell_rect(area, row_y).inner(Margin::new(0, COLLAPSED_AVATAR_VERTICAL_MARGIN))
}

fn expanded_avatar_rect(area: Rect, row_y: u16) -> Rect {
    let width = expanded_avatar_width(area.width);
    Rect {
        x: area.x + AVATAR_X_OFFSET,
        y: row_y,
        width,
        height: EXPANDED_ROW_HEIGHT.min(area.y + area.height - row_y),
    }
}

fn expanded_avatar_width(row_width: u16) -> u16 {
    let usable = row_width.saturating_sub(AVATAR_X_OFFSET.saturating_add(1));
    usable.clamp(1, EXPANDED_AVATAR_WIDTH)
}

fn expanded_text_x_offset(row_width: u16) -> u16 {
    AVATAR_X_OFFSET
        .saturating_add(expanded_avatar_width(row_width))
        .saturating_add(EXPANDED_TEXT_GAP)
}

fn row_height(collapsed: bool) -> u16 {
    if collapsed {
        COLLAPSED_ROW_HEIGHT
    } else {
        EXPANDED_ROW_HEIGHT
    }
}

fn visible_guild_rows<'a>(
    guilds: &'a [GuildSummary],
    state: &ListState,
    area: Rect,
    collapsed: bool,
) -> impl Iterator<Item = (u16, &'a GuildSummary)> {
    let row_height = row_height(collapsed);
    let offset = state.offset();
    let visible_rows = usize::from(area.height / row_height.max(1));
    let row_count = visible_rows
        .saturating_add(1)
        .min(guilds.len().saturating_sub(offset));

    guilds
        .iter()
        .skip(offset)
        .take(row_count)
        .enumerate()
        .filter_map(move |(row_idx, guild)| {
            let row_idx = u16::try_from(row_idx).ok()?;
            let row_y = area.y.saturating_add(row_idx.saturating_mul(row_height));
            (row_y < area.y + area.height).then_some((row_y, guild))
        })
}

fn blank_lines(count: u16) -> Vec<Line<'static>> {
    (0..count).map(|_| Line::from("")).collect()
}

fn title(
    guilds: &[GuildSummary],
    selected_guild_id: Option<&str>,
    loading_bar: Option<&str>,
) -> String {
    match selected_guild_id.and_then(|id| guilds.iter().find(|guild| guild.id == id)) {
        Some(selected) => format!(
            " Guilds {} · {}{} ",
            guilds.len(),
            truncate_name(&selected.name, 16),
            loading_bar.map_or_else(String::new, |bar| format!(" {bar}"))
        ),
        None => format!(
            " Guilds {}{} ",
            guilds.len(),
            loading_bar.map_or_else(String::new, |bar| format!(" {bar}"))
        ),
    }
}

fn render_selected_guild_preview(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    selected_guild_id: Option<&str>,
    avatars: &mut AvatarStore,
) {
    let Some(selected) =
        selected_guild_id.and_then(|id| guilds.iter().find(|guild| guild.id == id))
    else {
        return;
    };
    let [avatar_area, text_area] =
        Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)]).areas(area);
    avatars.render_avatar(
        frame,
        avatar_area,
        selected.avatar_url.as_deref(),
        &badge_from_name(&selected.name),
        ratatui::style::Color::Rgb(88, 101, 242),
        AvatarTone::FullColor,
    );
    let info = Paragraph::new(vec![
        Line::from(Span::styled(selected.name.clone(), theme::selected_item())),
        Line::from(if selected.muted {
            format!("muted · {} unread", selected.unread_count).dim()
        } else {
            format!("{} unread", selected.unread_count).dim()
        }),
    ]);
    frame.render_widget(info, text_area);
}

#[cfg(test)]
mod tests {
    use super::list_item;
    use crate::model::GuildSummary;
    use crate::ui::truncate_name;
    use ratatui::layout::Rect;

    fn guild() -> GuildSummary {
        GuildSummary {
            id: "g1".into(),
            name: "Hack Different".into(),
            muted: false,
            unread: false,
            unread_count: 0,
            avatar_url: None,
        }
    }

    #[test]
    fn truncate_name_handles_unicode_safely() {
        let name = "🦀 Rustaceans España";
        let truncated = truncate_name(name, 8);
        assert!(truncated.ends_with('…'));
        assert!(!truncated.is_empty());
    }

    #[test]
    fn truncate_name_keeps_short_unicode_names() {
        let name = "🦀 Rust";
        assert_eq!(truncate_name(name, 16), name);
    }

    #[test]
    fn collapsed_rows_render_avatar_only_placeholders() {
        let item = list_item(&guild(), 12, true);
        assert_eq!(item.height(), 5);
    }

    #[test]
    fn expanded_rows_have_multi_line_content() {
        let item = list_item(&guild(), 20, false);
        assert_eq!(item.height(), 3);
    }

    #[test]
    fn collapsed_avatar_rect_is_vertically_inset() {
        let area = Rect::new(0, 0, 5, 20);
        let rect = super::collapsed_avatar_rect(area, 3);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 4);
        assert_eq!(rect.width, 5);
        assert_eq!(rect.height, 3);
    }

    #[test]
    fn expanded_avatar_rect_is_wider_than_it_is_tall() {
        let area = Rect::new(0, 0, 18, 12);
        let rect = super::expanded_avatar_rect(area, 0);
        assert!(rect.width > rect.height);
    }
}
