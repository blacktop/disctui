use std::collections::{HashMap, HashSet};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui_image::StatefulImage;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;

#[derive(Debug, Clone, Copy)]
pub enum ImageSupport {
    Detected(ProtocolType),
    FallbackHalfblocks,
}

impl ImageSupport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Detected(ProtocolType::Kitty) => "kitty",
            Self::Detected(ProtocolType::Sixel) => "sixel",
            Self::Detected(ProtocolType::Iterm2) => "iterm2",
            Self::Detected(ProtocolType::Halfblocks) | Self::FallbackHalfblocks => "halfblocks",
        }
    }
}

#[expect(
    clippy::large_enum_variant,
    reason = "image protocol state is intentionally stored in-memory for fast rerender"
)]
enum AvatarStatus {
    Pending,
    Ready(StatefulProtocol),
    Failed,
}

pub struct AvatarStore {
    support: ImageSupport,
    picker: Picker,
    cache: HashMap<String, AvatarStatus>,
    inflight: HashSet<String>,
}

impl AvatarStore {
    pub fn detect() -> Self {
        match Picker::from_query_stdio() {
            Ok(mut picker) => {
                let protocol = picker.protocol_type();
                picker.set_protocol_type(protocol);
                Self {
                    support: ImageSupport::Detected(protocol),
                    picker,
                    cache: HashMap::new(),
                    inflight: HashSet::new(),
                }
            }
            Err(_) => Self::fallback(),
        }
    }

    pub fn fallback() -> Self {
        let picker = Picker::halfblocks();
        Self {
            support: ImageSupport::FallbackHalfblocks,
            picker,
            cache: HashMap::new(),
            inflight: HashSet::new(),
        }
    }

    pub fn protocol_label(&self) -> &'static str {
        self.support.label()
    }

    pub fn request(&mut self, url: &str) -> bool {
        if self.cache.contains_key(url) || !self.inflight.insert(url.to_string()) {
            return false;
        }
        self.cache.insert(url.to_string(), AvatarStatus::Pending);
        true
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "avatar bytes arrive owned from async action payloads"
    )]
    pub fn store_bytes(&mut self, url: String, bytes: Vec<u8>) {
        self.inflight.remove(&url);
        match image::load_from_memory(&bytes) {
            Ok(image) => {
                let protocol = self.picker.new_resize_protocol(image);
                self.cache.insert(url, AvatarStatus::Ready(protocol));
            }
            Err(_) => {
                self.cache.insert(url, AvatarStatus::Failed);
            }
        }
    }

    pub fn mark_failed(&mut self, url: String) {
        self.inflight.remove(&url);
        self.cache.insert(url, AvatarStatus::Failed);
    }

    pub fn render_avatar(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        url: Option<&str>,
        fallback_label: &str,
        fallback_color: ratatui::style::Color,
    ) {
        let Some(url) = url else {
            render_fallback(frame, area, fallback_label, fallback_color);
            return;
        };

        match self.cache.get_mut(url) {
            Some(AvatarStatus::Ready(protocol)) => {
                frame.render_stateful_widget(StatefulImage::new(), area, protocol);
            }
            Some(AvatarStatus::Pending) => {
                render_fallback(frame, area, "··", fallback_color);
            }
            Some(AvatarStatus::Failed) | None => {
                render_fallback(frame, area, fallback_label, fallback_color);
            }
        }
    }
}

fn render_fallback(frame: &mut Frame, area: Rect, label: &str, color: ratatui::style::Color) {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Span;

    // Colored background with white bold initials — mimics Discord's default avatar style
    let style = Style::new()
        .fg(ratatui::style::Color::White)
        .bg(color)
        .add_modifier(Modifier::BOLD);

    // Center the label vertically in the area
    let lines: Vec<Line> = (0..area.height)
        .map(|row| {
            if row == area.height / 2 {
                // Center text in the middle row
                let label_width = u16::try_from(label.len()).unwrap_or(area.width);
                let pad = area.width.saturating_sub(label_width) / 2;
                let padded = format!("{:>width$}{label}", "", width = pad as usize);
                let filled = format!("{padded:<width$}", width = area.width as usize);
                Line::from(Span::styled(filled, style))
            } else {
                // Fill row with background color
                Line::from(Span::styled(
                    " ".repeat(area.width as usize),
                    Style::new().bg(color),
                ))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

pub fn badge_from_name(name: &str) -> String {
    let mut badge = String::new();
    for ch in name.chars().filter(|c| c.is_alphanumeric()).take(2) {
        badge.push(ch.to_ascii_uppercase());
    }
    if badge.is_empty() {
        "??".to_string()
    } else {
        badge
    }
}
