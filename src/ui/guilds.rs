use crate::model::GuildSummary;
use crate::ui::media::{AvatarStore, badge_from_name};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    state: &mut ListState,
    focused: bool,
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
        .title(title(guilds, selected_guild_id).bold().cyan());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let show_preview = selected_guild_id.is_some() && inner.height >= 8;
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
        .map(|g| {
            let badge = badge_from_name(&g.name);
            let name = truncate_name(&g.name, list_area.width.saturating_sub(8) as usize);

            if g.unread {
                let mut spans = vec![
                    Span::styled("\u{25cf} ", theme::unread()),
                    Span::styled(
                        format!("{badge} {name}"),
                        Style::new()
                            .fg(ratatui::style::Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                if g.unread_count > 0 {
                    spans.push(Span::styled(
                        format!(" {}", g.unread_count),
                        theme::mention(),
                    ));
                }
                ListItem::new(Line::from(spans))
            } else {
                let label = format!("  {badge} {name}");
                ListItem::new(Line::from(label.dim()))
            }
        })
        .collect();

    let list = List::new(items)
        .highlight_style(theme::selected_item())
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, list_area, state);
}

fn title(guilds: &[GuildSummary], selected_guild_id: Option<&str>) -> String {
    match selected_guild_id.and_then(|id| guilds.iter().find(|g| g.id == id)) {
        Some(selected) => format!(
            " Guilds {} · {} ",
            guilds.len(),
            truncate_name(&selected.name, 16)
        ),
        None => format!(" Guilds {} ", guilds.len()),
    }
}

fn render_selected_guild_preview(
    frame: &mut Frame,
    area: Rect,
    guilds: &[GuildSummary],
    selected_guild_id: Option<&str>,
    avatars: &mut AvatarStore,
) {
    let Some(selected) = selected_guild_id.and_then(|id| guilds.iter().find(|g| g.id == id)) else {
        return;
    };
    let [avatar_area, text_area] =
        Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)]).areas(area);
    avatars.render_avatar(
        frame,
        avatar_area,
        selected.avatar_url.as_deref(),
        &badge_from_name(&selected.name),
        ratatui::style::Color::Rgb(88, 101, 242), // Discord blurple
    );
    let info = Paragraph::new(vec![
        Line::from(selected.name.clone().bold().cyan()),
        Line::from(format!("{} unread", selected.unread_count).dim()),
    ]);
    frame.render_widget(info, text_area);
}

use super::truncate_name;

#[cfg(test)]
mod tests {
    use crate::ui::truncate_name;

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
}
