//! One place for color and emphasis. Panes are separated by hairlines and
//! selection fills rather than boxes, so focus reads from the title and the
//! selected row instead of a border color.

use ratatui::style::{Color, Style};

pub const MUTE_GLYPH: &str = "⊘";
pub const UNREAD_GLYPH: &str = "●";

/// Brand accent: focus, unread markers, counts.
pub const ACCENT: Color = Color::Rgb(153, 162, 255);
/// Things addressed to the user: mentions and action items.
pub const ATTENTION: Color = Color::Rgb(232, 184, 107);
const DIM: Color = Color::Rgb(142, 147, 168);
const DANGER: Color = Color::Rgb(242, 139, 141);
const SUCCESS: Color = Color::Rgb(127, 207, 149);
/// Background of the top bar and footer.
const SURFACE: Color = Color::Rgb(26, 28, 36);
const HAIRLINE: Color = Color::Rgb(58, 62, 78);
const SELECTION_FOCUSED: Color = Color::Rgb(42, 46, 74);
const SELECTION: Color = Color::Rgb(34, 36, 46);
const ON_ACCENT: Color = Color::Rgb(20, 21, 27);

/// Title row of a pane. The focused pane's title carries the accent.
pub fn pane_title(focused: bool) -> Style {
    if focused {
        Style::new().fg(ACCENT).bold()
    } else {
        dim()
    }
}

/// Hairline between panes.
pub fn divider() -> Style {
    Style::new().fg(HAIRLINE)
}

/// Selected row of a list. Brighter in the focused pane.
pub fn selected_row(focused: bool) -> Style {
    if focused {
        Style::new().bg(SELECTION_FOCUSED).bold()
    } else {
        Style::new().bg(SELECTION)
    }
}

/// Emphasized text inside popups and the summary: keys, selected action items.
pub fn selected_item() -> Style {
    Style::new().fg(ACCENT).bold()
}

pub fn unread() -> Style {
    Style::new().fg(ACCENT).bold()
}

pub fn unread_name() -> Style {
    Style::new().bold()
}

pub fn count() -> Style {
    Style::new().fg(ACCENT)
}

pub fn attention() -> Style {
    Style::new().fg(ATTENTION).bold()
}

/// Top bar and footer background.
pub fn bar() -> Style {
    Style::new().bg(SURFACE)
}

pub fn brand_chip() -> Style {
    Style::new().fg(ON_ACCENT).bg(ACCENT).bold()
}

pub fn mode_chip() -> Style {
    Style::new().bg(HAIRLINE).bold()
}

pub fn key() -> Style {
    Style::new().bold()
}

pub fn title() -> Style {
    Style::new().fg(ACCENT).bold()
}

pub fn popup_border() -> Style {
    Style::new().fg(ACCENT)
}

pub fn error() -> Style {
    Style::new().fg(DANGER)
}

pub fn success() -> Style {
    Style::new().fg(SUCCESS)
}

pub fn new_messages_divider() -> Style {
    Style::new().fg(DANGER).bold()
}

/// Secondary text. A fixed color: ANSI dark gray is too faint to read on
/// many dark themes.
pub fn dim() -> Style {
    Style::new().fg(DIM)
}

pub fn muted() -> Style {
    dim()
}

pub fn mention() -> Style {
    attention()
}

pub fn link() -> Style {
    Style::new().light_blue().underlined()
}

pub fn code() -> Style {
    Style::new().green()
}

pub fn quote() -> Style {
    Style::new().light_magenta()
}

pub fn summary_header() -> Style {
    Style::new().fg(ACCENT).bold()
}

pub fn todo_bullet() -> Style {
    Style::new().yellow().bold()
}
