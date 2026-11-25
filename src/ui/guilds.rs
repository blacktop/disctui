use crate::action::PaneRow;
use crate::app::DigestPaneState;
use crate::model::GuildSummary;
use crate::ui::media::{AvatarStore, AvatarTone, badge_from_name};
use crate::ui::theme;
use crate::ui::truncate_name;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Stylize;
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
    digest: Option<&DigestPaneState>,
    mut hits: Option<&mut Vec<(Rect, PaneRow)>>,
) -> usize {
    if area.is_empty() {
        return 0;
    }
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(theme::divider())
        .title(title_line(guilds, focused, loading_bar, area.width));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let digest_selected = digest.is_some_and(|d| d.visible || (focused && d.rail_selected));
    let inner = if digest.is_some() {
        let [digest_area, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
        let label = if digest_area.width >= 8 {
            "◆ Digest"
        } else {
            " ◆"
        };
        let mut row = Paragraph::new(Span::styled(label, theme::attention()));
        if digest_selected {
            row = row.style(theme::selected_row(true));
        }
        frame.render_widget(row, digest_area);
        if let Some(hits) = hits.as_mut() {
            hits.push((digest_area, PaneRow::Digest));
        }
        rest
    } else {
        inner
    };

    if guilds.is_empty() {
        frame.render_widget(
            Paragraph::new("No servers loaded").style(theme::dim()),
            inner,
        );
        return 0;
    }

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

    let mut list = List::new(items);
    if !digest_selected {
        list = list.highlight_style(theme::selected_row(focused));
    }
    frame.render_stateful_widget(list, list_area, state);

    render_guild_avatars(
        frame,
        list_area,
        guilds,
        state,
        selected_guild_id,
        avatars,
        collapsed,
        hits,
    );
    let rows_per_page = usize::from(list_area.height / row_height(collapsed));
    guilds.len().saturating_sub(rows_per_page.max(1))
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
        theme::unread_name()
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
        primary.push(Span::styled(unread_suffix, theme::count()));
    }

    ListItem::new(vec![Line::from(""), Line::from(primary), Line::from("")])
}

#[expect(
    clippy::too_many_arguments,
    reason = "shares the list's visible avatar and click rows"
)]
fn render_guild_avatars(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    state: &ListState,
    selected_guild_id: Option<&str>,
    avatars: &mut AvatarStore,
    collapsed: bool,
    mut hits: Option<&mut Vec<(Rect, PaneRow)>>,
) {
    let height = row_height(collapsed);
    let rows = (area.y..area.bottom()).step_by(usize::from(height));
    for (row_y, (index, guild)) in rows.zip(guilds.iter().enumerate().skip(state.offset())) {
        if let Some(hits) = hits.as_mut() {
            hits.push((
                Rect::new(area.x, row_y, area.width, height.min(area.bottom() - row_y)),
                PaneRow::Guild(index),
            ));
        }
        let is_selected = selected_guild_id == Some(guild.id.as_str());
        if collapsed && is_selected {
            frame.render_widget(
                Paragraph::new("").style(theme::selected_row(false)),
                collapsed_avatar_cell_rect(area, row_y),
            );
        }
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
            theme::ACCENT,
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

fn blank_lines(count: u16) -> Vec<Line<'static>> {
    (0..count).map(|_| Line::from("")).collect()
}

/// "SERVERS n" when there is room; the collapsed avatar rail has no room for a label.
fn title_line(
    guilds: &[GuildSummary],
    focused: bool,
    loading_bar: Option<&str>,
    width: u16,
) -> Line<'static> {
    let label = if width >= 16 {
        format!(" SERVERS {}", guilds.len())
    } else {
        String::new()
    };
    let mut spans = vec![Span::styled(label, theme::pane_title(focused))];
    if let Some(bar) = loading_bar {
        spans.push(Span::styled(format!(" {bar}"), theme::count()));
    }
    Line::from(spans)
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
    use crate::model::GuildSummary;
    use crate::ui::guilds::{collapsed_avatar_rect, expanded_avatar_rect, list_item};
    use crate::ui::truncate_name;
    use ratatui::layout::Rect;

    fn guild() -> GuildSummary {
        GuildSummary {
            id: "g1".into(),
            name: "Rust Lang".into(),
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
        assert_ne!(truncated, "");
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
        let rect = collapsed_avatar_rect(area, 3);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 4);
        assert_eq!(rect.width, 5);
        assert_eq!(rect.height, 3);
    }

    #[test]
    fn expanded_avatar_rect_is_wider_than_it_is_tall() {
        let area = Rect::new(0, 0, 18, 12);
        let rect = expanded_avatar_rect(area, 0);
        assert!(rect.width > rect.height);
    }
}
