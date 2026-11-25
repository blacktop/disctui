use std::collections::{HashMap, HashSet, VecDeque};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

use crate::avatar_cache::{self, AvatarCache};

const MAX_AVATAR_DOWNLOADS: usize = 8;
const MAX_QUEUED_AVATARS: usize = 1024;

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
    picker: Picker,
    cache: HashMap<String, AvatarStatus>,
    disk: Option<AvatarCache>,
    queued: VecDeque<String>,
    active_downloads: usize,
}

impl AvatarStore {
    pub fn detect() -> Self {
        let mut store = Self::fallback();
        if let Ok(picker) = Picker::from_query_stdio() {
            store.picker = picker;
        }
        store.disk = AvatarCache::open_default();
        store
    }

    pub fn fallback() -> Self {
        Self {
            picker: Picker::halfblocks(),
            cache: HashMap::new(),
            disk: None,
            queued: VecDeque::new(),
            active_downloads: 0,
        }
    }

    pub fn protocol_label(&self) -> &'static str {
        match self.picker.protocol_type() {
            ProtocolType::Kitty => "kitty",
            ProtocolType::Sixel => "sixel",
            ProtocolType::Iterm2 => "iterm2",
            ProtocolType::Halfblocks => "halfblocks",
        }
    }

    /// Return true when a download should start immediately.
    pub fn request(&mut self, url: &str) -> bool {
        if self.cache.contains_key(url) {
            return false;
        }
        let Some(key) = avatar_cache::key(url) else {
            self.cache.insert(url.to_string(), AvatarStatus::Pending);
            return true;
        };
        if let Some(image) = self.disk.as_ref().and_then(|disk| disk.get(key)) {
            self.store_image(url.to_string(), image);
            return false;
        }
        if self.active_downloads >= MAX_AVATAR_DOWNLOADS {
            if self.queued.len() < MAX_QUEUED_AVATARS {
                self.cache.insert(url.to_string(), AvatarStatus::Pending);
                self.queued.push_back(url.to_string());
            }
            return false;
        }
        self.active_downloads += 1;
        self.cache.insert(url.to_string(), AvatarStatus::Pending);
        true
    }

    pub fn finish_request(&mut self, url: String, bytes: Option<&[u8]>) -> Option<String> {
        let key = avatar_cache::key(&url);
        let is_avatar = key.is_some();
        let image = bytes.and_then(|bytes| {
            let Some(key) = key else {
                return image::load_from_memory(bytes).ok();
            };
            let image = avatar_cache::decode_cached(bytes)?;
            if let Some(disk) = &mut self.disk
                && let Err(err) = disk.put(key, bytes)
            {
                tracing::debug!("could not save avatar to cache: {err}");
            }
            Some(image)
        });
        if let Some(image) = image {
            self.store_image(url, image);
        } else {
            self.cache.insert(url, AvatarStatus::Failed);
        }
        if !is_avatar {
            return None;
        }
        let next = self.queued.pop_front();
        if next.is_none() {
            self.active_downloads = self.active_downloads.saturating_sub(1);
        }
        next
    }

    pub fn prioritize_queued<'a>(&mut self, urls: impl IntoIterator<Item = &'a str>) {
        if self.queued.len() < 2 {
            return;
        }
        let current: HashSet<_> = urls.into_iter().collect();
        self.queued
            .make_contiguous()
            .sort_by_key(|url| !current.contains(url.as_str()));
    }

    fn store_image(&mut self, url: String, image: image::DynamicImage) {
        let muted = self.picker.new_resize_protocol(image.grayscale());
        let color = self.picker.new_resize_protocol(image);
        self.cache.insert(url, AvatarStatus::Ready { color, muted });
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
        if area.is_empty() {
            return;
        }
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
            Some(AvatarStatus::Pending) if avatar_cache::key(url).is_none() => {
                render_fallback(frame, area, "··", fallback_color, tone);
            }
            Some(AvatarStatus::Pending | AvatarStatus::Failed) | None => {
                render_fallback(frame, area, fallback_label, fallback_color, tone);
            }
        }
    }
}

fn centered_render_area(protocol: &StatefulProtocol, area: Rect) -> Rect {
    let fitted = protocol.size_for(Resize::Fit(None), area.into());
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

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test cache and fixtures must initialize"
)]
mod tests {
    use std::io::Cursor;

    use crate::ui::media::{AvatarCache, AvatarStatus, AvatarStore, MAX_AVATAR_DOWNLOADS};

    #[test]
    fn saved_avatar_is_ready_without_a_download_after_reopen() {
        let dir = std::env::temp_dir().join(format!("disctui-avatar-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let cleanup = std::mem::DropGuard::new(dir.clone(), |dir| {
            let _ = std::fs::remove_dir_all(dir);
        });
        let mut store = AvatarStore::fallback();
        store.disk = Some(AvatarCache::open(&dir).unwrap());
        let url = "https://cdn.discordapp.com/avatars/123/abcd.png?size=64";
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(64, 64)
            .write_to(&mut source, image::ImageFormat::Png)
            .unwrap();
        let png = crate::avatar_cache::normalize(&source.into_inner()).unwrap();
        assert!(store.request(url));
        assert!(store.finish_request(url.to_string(), Some(&png)).is_none());
        drop(store);

        let mut store = AvatarStore::fallback();
        store.disk = Some(AvatarCache::open(&dir).unwrap());
        assert!(!store.request(url));
        assert!(matches!(
            store.cache.get(url),
            Some(AvatarStatus::Ready { .. })
        ));
        assert!(store.request("https://cdn.discordapp.com/avatars/123/newhash.png?size=64"));
        drop(store);
        drop(cleanup);
    }

    #[test]
    fn failed_download_releases_a_slot_to_the_next_unique_avatar() {
        let mut store = AvatarStore::fallback();
        let url = |id| format!("https://cdn.discordapp.com/avatars/{id}/abcd.png?size=64");
        for id in 0..MAX_AVATAR_DOWNLOADS {
            assert!(store.request(&url(id)));
        }
        let queued = url(MAX_AVATAR_DOWNLOADS);
        assert!(!store.request(&queued));
        assert!(!store.request(&queued));
        assert_eq!(store.finish_request(url(0), None), Some(queued.clone()));
        assert!(store.finish_request(queued, None).is_none());
        assert!(store.request(&url(MAX_AVATAR_DOWNLOADS + 1)));
    }
}
