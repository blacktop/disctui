use ratatui::style::{Color, Modifier, Style};

pub const MUTE_GLYPH: &str = "⊘";
pub const DISCORD_BLURPLE: Color = Color::Rgb(88, 101, 242);
pub const DISCORD_BLURPLE_LIGHT: Color = Color::Rgb(121, 134, 255);
pub const DISCORD_BG_DARK: Color = Color::Rgb(35, 39, 42);

// Pane border when focused
pub fn focused_border() -> Style {
    Style::new().fg(DISCORD_BLURPLE)
}

// Pane border when not focused
pub fn unfocused_border() -> Style {
    Style::new().fg(Color::DarkGray)
}

// Selected item in a list
pub fn selected_item() -> Style {
    Style::new()
        .fg(DISCORD_BLURPLE_LIGHT)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_panel_row() -> Style {
    Style::new()
        .fg(Color::White)
        .bg(Color::Rgb(78, 88, 150))
        .add_modifier(Modifier::BOLD)
}

pub fn selected_avatar_only_row() -> Style {
    Style::new().bg(Color::Rgb(67, 76, 130))
}

// Normal list item
#[expect(dead_code, reason = "reserved for unselected list items")]
pub fn normal_item() -> Style {
    Style::new().fg(Color::Gray)
}

// Unread indicator
pub fn unread() -> Style {
    Style::new()
        .fg(DISCORD_BLURPLE_LIGHT)
        .add_modifier(Modifier::BOLD)
}

// Status bar background
pub fn status_bar() -> Style {
    Style::new().fg(Color::White).bg(DISCORD_BG_DARK)
}

pub fn app_badge() -> Style {
    Style::new()
        .fg(Color::White)
        .bg(DISCORD_BLURPLE)
        .add_modifier(Modifier::BOLD)
}

pub fn title() -> Style {
    Style::new()
        .fg(DISCORD_BLURPLE_LIGHT)
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
    Style::new()
        .fg(DISCORD_BLURPLE_LIGHT)
        .add_modifier(Modifier::BOLD)
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
    Style::new()
        .fg(DISCORD_BLURPLE_LIGHT)
        .add_modifier(Modifier::BOLD)
}

// TODO bullet
pub fn todo_bullet() -> Style {
    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
}
