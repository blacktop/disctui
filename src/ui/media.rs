use std::collections::{HashMap, HashSet};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

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
    Ready {
        color: StatefulProtocol,
        muted: StatefulProtocol,
    },
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarTone {
    FullColor,
    Muted,
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
                let muted_image = image.grayscale();
                let color = self.picker.new_resize_protocol(image);
                let muted = self.picker.new_resize_protocol(muted_image);
                self.cache.insert(url, AvatarStatus::Ready { color, muted });
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
        tone: AvatarTone,
    ) {
        let Some(url) = url else {
            render_fallback(frame, area, fallback_label, fallback_color, tone);
            return;
        };

        match self.cache.get_mut(url) {
            Some(AvatarStatus::Ready { color, muted }) => {
                let protocol = match tone {
                    AvatarTone::FullColor => color,
                    AvatarTone::Muted => muted,
                };
                let render_area = centered_render_area(protocol, area);
                frame.render_stateful_widget(
                    StatefulImage::new().resize(Resize::Fit(None)),
                    render_area,
                    protocol,
                );
            }
            Some(AvatarStatus::Pending) => {
                render_fallback(frame, area, "··", fallback_color, tone);
            }
            Some(AvatarStatus::Failed) | None => {
                render_fallback(frame, area, fallback_label, fallback_color, tone);
            }
        }
    }
}

fn centered_render_area(protocol: &StatefulProtocol, area: Rect) -> Rect {
    let fitted = protocol.size_for(Resize::Fit(None), area);
    let width = fitted.width.min(area.width);
    let height = fitted.height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn render_fallback(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    color: ratatui::style::Color,
    tone: AvatarTone,
) {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Span;

    let foreground = match tone {
        AvatarTone::FullColor => color,
        AvatarTone::Muted => desaturate_color(color),
    };

    let style = Style::new().fg(foreground).add_modifier(Modifier::BOLD);

    let lines: Vec<Line> = (0..area.height)
        .map(|row| {
            if row == area.height / 2 {
                let label_width = u16::try_from(label.len()).unwrap_or(area.width);
                let pad = area.width.saturating_sub(label_width) / 2;
                let padded = format!("{:>width$}{label}", "", width = pad as usize);
                let filled = format!("{padded:<width$}", width = area.width as usize);
                Line::from(Span::styled(filled, style))
            } else {
                Line::from(" ".repeat(area.width as usize))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn desaturate_color(color: ratatui::style::Color) -> ratatui::style::Color {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => {
            let gray = ((u32::from(r) * 299) + (u32::from(g) * 587) + (u32::from(b) * 114)) / 1000;
            let gray = u8::try_from(gray).unwrap_or(u8::MAX);
            ratatui::style::Color::Rgb(gray, gray, gray)
        }
        ratatui::style::Color::Indexed(index) => ratatui::style::Color::Indexed(index),
        _ => ratatui::style::Color::DarkGray,
    }
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
