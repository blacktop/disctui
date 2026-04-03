use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::FocusPane;

/// Top-level layout: body + status bar.
pub fn root(area: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area)
}

/// Three-pane horizontal split: guilds | channels | main content.
///
/// The sidebar widths adapt to focus and whether a channel is open so the
/// active pane gets more space and the message view dominates once you are
/// reading a channel.
pub fn main_body(area: Rect, focus: FocusPane, has_selected_channel: bool) -> [Rect; 3] {
    let (guild_width, channel_width) = pane_widths(area.width, focus, has_selected_channel);
    Layout::horizontal([
        Constraint::Length(guild_width),
        Constraint::Length(channel_width),
        Constraint::Fill(1),
    ])
    .areas(area)
}

/// Message pane split: messages | input.
pub fn message_pane(area: Rect, input_height: u16) -> [Rect; 2] {
    Layout::vertical([Constraint::Fill(1), Constraint::Length(input_height)]).areas(area)
}

/// Centered popup for help/error overlays.
pub fn centered_popup(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, center_v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(center_v);

    center
}

fn pane_widths(total_width: u16, focus: FocusPane, has_selected_channel: bool) -> (u16, u16) {
    let (guild_pct, channel_pct, guild_min, guild_max, channel_min, channel_max, main_min): (
        u16,
        u16,
        u16,
        u16,
        u16,
        u16,
        u16,
    ) = match (focus, has_selected_channel) {
        (FocusPane::Guilds, _) => (28, 24, 18, 30, 18, 28, 34),
        (FocusPane::Channels, _) => (16, 30, 12, 22, 20, 34, 34),
        (FocusPane::Messages | FocusPane::Input, true) => (12, 18, 10, 16, 14, 24, 44),
        _ => (16, 24, 12, 20, 18, 28, 36),
    };

    let mut guild =
        u16::try_from((u32::from(total_width) * u32::from(guild_pct)) / 100).unwrap_or(u16::MAX);
    let mut channel =
        u16::try_from((u32::from(total_width) * u32::from(channel_pct)) / 100).unwrap_or(u16::MAX);

    guild = guild.clamp(guild_min, guild_max.min(total_width));
    channel = channel.clamp(
        channel_min,
        channel_max.min(total_width.saturating_sub(guild)),
    );

    let mut remaining_for_main = total_width.saturating_sub(guild.saturating_add(channel));
    if remaining_for_main < main_min {
        let mut overflow = main_min - remaining_for_main;

        let shrink_channel = overflow.min(channel.saturating_sub(channel_min));
        channel -= shrink_channel;
        overflow -= shrink_channel;

        let shrink_guild = overflow.min(guild.saturating_sub(guild_min));
        guild -= shrink_guild;
        overflow -= shrink_guild;

        if overflow > 0 {
            channel = channel.saturating_sub(overflow.min(channel.saturating_sub(8)));
        }

        remaining_for_main = total_width.saturating_sub(guild.saturating_add(channel));
        if remaining_for_main == 0 {
            channel = channel.saturating_sub(1);
        }
    }

    (guild, channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_splits_body_and_status() {
        let area = Rect::new(0, 0, 80, 24);
        let [body, status] = root(area);
        assert_eq!(status.height, 1);
        assert_eq!(body.height, 23);
        assert_eq!(body.width, 80);
    }

    #[test]
    fn main_body_three_panes() {
        let area = Rect::new(0, 0, 80, 23);
        let [guilds, channels, main] = main_body(area, FocusPane::Messages, true);
        assert_eq!(guilds.width + channels.width + main.width, 80);
        assert!(main.width > channels.width);
    }

    #[test]
    fn narrow_terminal_still_renders() {
        // Terminal narrower than guild + channel sidebars
        let area = Rect::new(0, 0, 30, 10);
        let [body, status] = root(area);
        assert_eq!(status.height, 1);

        let [guilds, channels, main] = main_body(body, FocusPane::Messages, true);
        // Ratatui clamps — sidebars take priority, main gets remainder
        assert!(guilds.width > 0);
        assert!(channels.width > 0);
        // Main pane may be 0 width in very narrow terminals
        assert_eq!(guilds.width + channels.width + main.width, body.width);
    }

    #[test]
    fn very_short_terminal() {
        let area = Rect::new(0, 0, 80, 3);
        let [body, status] = root(area);
        assert_eq!(status.height, 1);
        assert_eq!(body.height, 2);
    }

    #[test]
    fn message_pane_splits_content_and_input() {
        let area = Rect::new(0, 0, 44, 20);
        let [messages, input] = message_pane(area, 3);
        assert_eq!(input.height, 3);
        assert_eq!(messages.height, 17);
    }

    #[test]
    fn centered_popup_is_centered() {
        let area = Rect::new(0, 0, 100, 50);
        let popup = centered_popup(60, 60, area);
        // Should be roughly centered
        assert!(popup.x > 0);
        assert!(popup.y > 0);
        assert!(popup.x + popup.width <= area.width);
        assert!(popup.y + popup.height <= area.height);
    }

    #[test]
    fn messages_focus_gives_main_pane_majority_width() {
        let area = Rect::new(0, 0, 120, 30);
        let [guilds, channels, main] = main_body(area, FocusPane::Messages, true);
        assert!(main.width > guilds.width + channels.width);
    }

    #[test]
    fn channel_focus_expands_channel_sidebar() {
        let area = Rect::new(0, 0, 120, 30);
        let [guilds, channels, _main] = main_body(area, FocusPane::Channels, true);
        assert!(channels.width > guilds.width);
    }
}
