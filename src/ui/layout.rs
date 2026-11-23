use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::FocusPane;

/// Top-level layout: top bar, body, footer.
pub fn root(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

/// Three-pane horizontal split: guilds | channels | main content.
///
/// Narrow screens show the focused pane. Wider screens progressively restore
/// the avatar rail and channel list.
pub fn main_body(area: Rect, focus: FocusPane) -> [Rect; 3] {
    let (guild_width, channel_width) = pane_widths(area.width, focus);
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

fn pane_widths(total_width: u16, focus: FocusPane) -> (u16, u16) {
    if total_width < 72 {
        return match focus {
            FocusPane::Guilds => (total_width, 0),
            FocusPane::Channels => (0, total_width),
            FocusPane::Messages | FocusPane::Input => (0, 0),
        };
    }
    if total_width < 105 {
        return match focus {
            FocusPane::Guilds => (30.min(total_width), total_width.saturating_sub(30)),
            FocusPane::Channels => (7, total_width.saturating_sub(7)),
            FocusPane::Messages | FocusPane::Input => (7, 0),
        };
    }
    let channel = (total_width / 5).clamp(22, 30);
    match focus {
        FocusPane::Guilds => (30, channel),
        FocusPane::Channels => (7, channel.max(28)),
        FocusPane::Messages | FocusPane::Input => (7, channel),
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::layout::*;

    #[test]
    fn root_splits_top_bar_body_and_footer() {
        let area = Rect::new(0, 0, 80, 24);
        let [top, body, footer] = root(area);
        assert_eq!(top.height, 1);
        assert_eq!(footer.height, 1);
        assert_eq!(body.height, 22);
        assert_eq!(body.width, 80);
        assert_eq!(footer.y, 23);
    }

    #[test]
    fn main_body_three_panes() {
        let area = Rect::new(0, 0, 80, 23);
        let [guilds, channels, main] = main_body(area, FocusPane::Messages);
        assert_eq!(guilds.width + channels.width + main.width, 80);
        assert!(main.width > channels.width);
    }

    #[test]
    fn narrow_terminal_still_renders() {
        for width in [1, 30, 71] {
            let area = Rect::new(0, 0, width, 10);
            assert_eq!(
                main_body(area, FocusPane::Guilds).map(|r| r.width),
                [width, 0, 0]
            );
            assert_eq!(
                main_body(area, FocusPane::Channels).map(|r| r.width),
                [0, width, 0]
            );
            assert_eq!(
                main_body(area, FocusPane::Messages).map(|r| r.width),
                [0, 0, width]
            );
        }
    }

    #[test]
    fn very_short_terminal() {
        let area = Rect::new(0, 0, 80, 3);
        let [top, body, footer] = root(area);
        assert_eq!(top.height, 1);
        assert_eq!(footer.height, 1);
        assert_eq!(body.height, 1);
    }

    #[test]
    fn message_pane_splits_content_and_input() {
        let area = Rect::new(0, 0, 44, 20);
        let [messages, input] = message_pane(area, 3);
        assert_eq!(input.height, 3);
        assert_eq!(messages.height, 17);
    }

    #[test]
    fn panes_never_exceed_a_terminal_narrower_than_their_minimums() {
        for width in 0..40 {
            for focus in [
                FocusPane::Guilds,
                FocusPane::Channels,
                FocusPane::Messages,
                FocusPane::Input,
            ] {
                let area = Rect::new(0, 0, width, 10);
                let [guilds, channels, main] = main_body(area, focus);
                assert_eq!(guilds.width + channels.width + main.width, width);
            }
        }
    }

    #[test]
    fn messages_focus_gives_main_pane_majority_width() {
        let area = Rect::new(0, 0, 120, 30);
        let [guilds, channels, main] = main_body(area, FocusPane::Messages);
        assert!(main.width > guilds.width + channels.width);
    }

    #[test]
    fn channel_focus_expands_channel_sidebar() {
        let area = Rect::new(0, 0, 120, 30);
        let [guilds, channels, _main] = main_body(area, FocusPane::Channels);
        assert!(channels.width > guilds.width);
    }
}
