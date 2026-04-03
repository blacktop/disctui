use ratatui::style::{Color, Modifier, Style};

pub const MUTE_GLYPH: &str = "⊘";

// Pane border when focused
pub fn focused_border() -> Style {
    Style::new().fg(Color::Cyan)
}

// Pane border when not focused
pub fn unfocused_border() -> Style {
    Style::new().fg(Color::DarkGray)
}

// Selected item in a list
pub fn selected_item() -> Style {
    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

// Normal list item
#[expect(dead_code, reason = "reserved for unselected list items")]
pub fn normal_item() -> Style {
    Style::new().fg(Color::Gray)
}

// Unread indicator
pub fn unread() -> Style {
    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

// Status bar background
pub fn status_bar() -> Style {
    Style::new().fg(Color::White).bg(Color::DarkGray)
}

// Mode indicator in status bar
pub fn mode_normal() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn mode_insert() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

// Error text
pub fn error() -> Style {
    Style::new().fg(Color::Red)
}

// Dimmed / secondary text
pub fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn muted() -> Style {
    dim()
}

pub fn mention() -> Style {
    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

pub fn link() -> Style {
    Style::new()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::UNDERLINED)
}

pub fn code() -> Style {
    Style::new().fg(Color::Green)
}

pub fn quote() -> Style {
    Style::new().fg(Color::LightMagenta)
}

// Summary header
pub fn summary_header() -> Style {
    Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)
}

// TODO bullet
pub fn todo_bullet() -> Style {
    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
}
