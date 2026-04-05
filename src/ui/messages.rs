use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use textwrap::wrap;
use unicode_width::UnicodeWidthStr;

use crate::model::MessageRow;
use crate::ui::media::{AvatarStore, AvatarTone, badge_from_name};
use crate::ui::theme;

const AVATAR_GUTTER_WIDTH: u16 = 6;
const AVATAR_HEIGHT: u16 = 2;
const IMAGE_CARD_HEIGHT: u16 = 6;
const GROUP_SPACING: usize = 1;

struct MessageLayoutPlan {
    lines: Vec<Line<'static>>,
    avatars: Vec<AvatarMarker>,
    images: Vec<ImageMarker>,
}

struct AvatarMarker {
    line_index: u16,
    url: Option<String>,
    fallback: String,
    /// Color for the fallback badge (derived from author name).
    fallback_color: Color,
}

struct ImageMarker {
    line_index: u16,
    url: String,
    filename: String,
}

#[expect(
    clippy::too_many_arguments,
    reason = "message pane renderer takes viewport, loading, unread-divider, and avatar state"
)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    messages: &[MessageRow],
    unread_divider_message_id: Option<&str>,
    scroll_offset: u16,
    loading_bar: Option<&str>,
    focused: bool,
    avatars: &mut AvatarStore,
) -> u16 {
    let border_style = if focused {
        theme::focused_border()
    } else {
        theme::unfocused_border()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            loading_bar.map_or_else(
                || " Messages ".to_string(),
                |bar| format!(" Messages {bar} "),
            ),
            theme::title(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text_area = Rect {
        x: inner.x + AVATAR_GUTTER_WIDTH.min(inner.width),
        y: inner.y,
        width: inner.width.saturating_sub(AVATAR_GUTTER_WIDTH),
        height: inner.height,
    };
    let avatar_area = Rect {
        x: inner.x,
        y: inner.y,
        width: AVATAR_GUTTER_WIDTH.min(inner.width),
        height: inner.height,
    };

    let plan = build_message_layout(messages, text_area.width, unread_divider_message_id);
    let total_lines = u16::try_from(plan.lines.len()).unwrap_or(u16::MAX);
    let viewport_height = text_area.height;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    // Clamp scroll offset so caller can pass u16::MAX for "scroll to bottom"
    let clamped_offset = scroll_offset.min(max_scroll);

    let paragraph = Paragraph::new(plan.lines).scroll((clamped_offset, 0));

    frame.render_widget(paragraph, text_area);
    render_avatars(frame, avatar_area, clamped_offset, avatars, &plan.avatars);
    render_images(frame, text_area, clamped_offset, avatars, &plan.images);
    max_scroll
}

fn build_message_layout(
    messages: &[MessageRow],
    text_width: u16,
    unread_divider_message_id: Option<&str>,
) -> MessageLayoutPlan {
    if messages.is_empty() {
        return MessageLayoutPlan {
            lines: vec![Line::from(Span::styled("No messages yet", theme::dim()))],
            avatars: Vec::new(),
            images: Vec::new(),
        };
    }

    let mut lines = Vec::new();
    let mut avatars = Vec::new();
    let mut images = Vec::new();

    let wrap_width = usize::from(text_width.saturating_sub(2).max(8));

    for (idx, msg) in messages.iter().enumerate() {
        if unread_divider_message_id == Some(msg.id.as_str()) {
            lines.push(new_messages_divider_line(text_width));
        }

        let group_start = lines.len();
        if !msg.is_continuation {
            let line_index = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            avatars.push(AvatarMarker {
                line_index,
                url: msg.author_avatar_url.clone(),
                fallback: badge_from_name(&msg.author),
                fallback_color: discord_default_avatar_color(&msg.author),
            });
            let mut header_spans = vec![
                Span::styled(msg.author.clone(), author_name_style(&msg.author)),
                Span::raw("  "),
                Span::styled(msg.timestamp.clone(), theme::dim()),
            ];
            if msg.edited {
                header_spans.push(Span::styled(" (edited)", theme::dim()));
            }
            lines.push(Line::from(header_spans));
        }

        for content_line in msg.content.lines() {
            let wrapped = wrap_content_line(content_line, wrap_width);
            for wrapped_line in wrapped {
                lines.push(Line::from(styled_content_line(&wrapped_line)));
            }
        }

        for attachment in &msg.attachments {
            if attachment.is_image {
                lines.push(Line::from(Span::styled(
                    format!("attachment: {}", attachment.filename),
                    theme::dim(),
                )));
                let line_index = u16::try_from(lines.len()).unwrap_or(u16::MAX);
                images.push(ImageMarker {
                    line_index,
                    url: attachment.url.clone(),
                    filename: attachment.filename.clone(),
                });
                for _ in 0..IMAGE_CARD_HEIGHT {
                    lines.push(Line::from(Span::raw("")));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    format!("attachment: {}", attachment.filename),
                    theme::link(),
                )));
            }
        }

        if !msg.is_continuation {
            let group_lines = lines.len().saturating_sub(group_start);
            if group_lines < usize::from(AVATAR_HEIGHT) {
                for _ in 0..(usize::from(AVATAR_HEIGHT) - group_lines) {
                    lines.push(Line::from(Span::raw("")));
                }
            }
        }

        let next_starts_new_group = messages
            .get(idx + 1)
            .is_none_or(|next| !next.is_continuation);
        if next_starts_new_group {
            for _ in 0..GROUP_SPACING {
                lines.push(Line::from(Span::raw("")));
            }
        }
    }

    MessageLayoutPlan {
        lines,
        avatars,
        images,
    }
}

fn new_messages_divider_line(text_width: u16) -> Line<'static> {
    let width = usize::from(text_width.max(12));
    let label = " New Messages ";
    let label_width = label.width();
    if width <= label_width {
        return Line::from(Span::styled(label, theme::new_messages_divider()));
    }

    let side_width = (width.saturating_sub(label_width)) / 2;
    let left = "─".repeat(side_width);
    let right = "─".repeat(width.saturating_sub(label_width).saturating_sub(side_width));

    Line::from(vec![
        Span::styled(left, theme::new_messages_divider()),
        Span::styled(label, theme::new_messages_divider()),
        Span::styled(right, theme::new_messages_divider()),
    ])
}

fn wrap_content_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    wrap(line, width)
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect()
}

fn styled_content_line(line: &str) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled("  ", theme::dim())];
    let trimmed = line.trim_start();

    if trimmed.starts_with("```") {
        spans.push(Span::styled(trimmed.to_string(), theme::code()));
        return spans;
    }

    if trimmed.starts_with('>') {
        spans.push(Span::styled(trimmed.to_string(), theme::quote()));
        return spans;
    }

    for segment in line.split_inclusive(char::is_whitespace) {
        let token = segment.trim_end_matches(char::is_whitespace);
        let whitespace = &segment[token.len()..];

        if !token.is_empty() {
            spans.push(styled_token(token));
        }
        if !whitespace.is_empty() {
            spans.push(Span::raw(whitespace.to_string()));
        }
    }

    spans
}

fn styled_token(token: &str) -> Span<'static> {
    let style = if token.starts_with('@') {
        Some(theme::mention())
    } else if token.starts_with("http://") || token.starts_with("https://") {
        Some(theme::link())
    } else if token.starts_with('`') && token.ends_with('`') && token.len() >= 2 {
        Some(theme::code())
    } else if token.starts_with('-') || token.starts_with('*') {
        Some(theme::todo_bullet())
    } else {
        None
    };

    if let Some(style) = style {
        Span::styled(token.to_string(), style)
    } else {
        Span::raw(token.to_string())
    }
}

fn author_name_style(author: &str) -> Style {
    let palette = [
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::LightBlue,
        Color::LightGreen,
        Color::LightMagenta,
    ];
    let idx = author.bytes().fold(0u8, u8::wrapping_add) as usize % palette.len();
    Style::new()
        .fg(palette[idx])
        .add_modifier(ratatui::style::Modifier::BOLD)
}

/// Discord's default avatar color palette (used when a user has no custom avatar).
/// Computed from user ID as `(id >> 22) % 6`, but we use author name hash since
/// the UI layer doesn't have the raw user ID.
fn discord_default_avatar_color(author: &str) -> Color {
    const DISCORD_COLORS: [Color; 6] = [
        Color::Rgb(88, 101, 242),  // blurple
        Color::Rgb(117, 117, 117), // gray
        Color::Rgb(59, 165, 92),   // green
        Color::Rgb(250, 166, 26),  // yellow
        Color::Rgb(237, 66, 69),   // red
        Color::Rgb(235, 69, 158),  // fuchsia
    ];
    let idx = author.bytes().fold(0u8, u8::wrapping_add) as usize % DISCORD_COLORS.len();
    DISCORD_COLORS[idx]
}

fn render_avatars(
    frame: &mut Frame,
    area: Rect,
    scroll_offset: u16,
    avatars: &mut AvatarStore,
    markers: &[AvatarMarker],
) {
    for marker in markers {
        if marker.line_index < scroll_offset {
            continue;
        }
        let y = area.y + marker.line_index.saturating_sub(scroll_offset);
        if y >= area.y + area.height {
            continue;
        }
        let avatar_height = AVATAR_HEIGHT.min(area.y + area.height - y);
        let avatar_rect = Rect {
            x: area.x,
            y,
            width: area.width.saturating_sub(1),
            height: avatar_height,
        };
        avatars.render_avatar(
            frame,
            avatar_rect,
            marker.url.as_deref(),
            &marker.fallback,
            marker.fallback_color,
            AvatarTone::FullColor,
        );
    }
}

fn render_images(
    frame: &mut Frame,
    area: Rect,
    scroll_offset: u16,
    avatars: &mut AvatarStore,
    markers: &[ImageMarker],
) {
    for marker in markers {
        if marker.line_index < scroll_offset {
            continue;
        }
        let y = area.y + marker.line_index.saturating_sub(scroll_offset);
        if y >= area.y + area.height {
            continue;
        }
        let image_height = IMAGE_CARD_HEIGHT.min(area.y + area.height - y);
        let image_rect = Rect {
            x: area.x,
            y,
            width: area.width.min(40),
            height: image_height,
        };
        avatars.render_avatar(
            frame,
            image_rect,
            Some(&marker.url),
            &badge_from_name(&marker.filename),
            Color::DarkGray,
            AvatarTone::FullColor,
        );
    }
}
