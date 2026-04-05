use std::collections::HashMap;

pub const DIRECT_MESSAGES_GUILD_ID: &str = "@me";
pub const DIRECT_MESSAGES_GUILD_NAME: &str = "Direct Messages";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Category,
    Text,
    #[cfg_attr(not(feature = "experimental-discord"), expect(dead_code))]
    Announcement,
    DirectMessage,
}

impl ChannelKind {
    pub const fn is_selectable(self) -> bool {
        !matches!(self, Self::Category)
    }

    pub const fn marker(self) -> &'static str {
        match self {
            Self::Category => "",
            Self::Text | Self::Announcement => "#",
            Self::DirectMessage => "@",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuildSummary {
    pub id: String,
    pub name: String,
    pub muted: bool,
    pub unread: bool,
    pub unread_count: u32,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelSummary {
    pub id: String,
    pub guild_id: Option<String>,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: ChannelKind,
    pub position: i32,
    pub muted: bool,
    pub unread: bool,
    pub unread_count: u32,
    /// Last message ID in this channel (from Discord), used for mark-all-read persistence.
    pub last_message_id: Option<String>,
}

impl ChannelSummary {
    pub const fn is_effectively_muted(&self, guild_muted: bool) -> bool {
        guild_muted || self.muted
    }

    pub const fn shows_unread_in_guild_rollup(&self, guild_muted: bool) -> bool {
        self.unread && !self.is_effectively_muted(guild_muted)
    }

    pub const fn shows_unread_in_channel_list(&self) -> bool {
        self.unread && !self.muted
    }
}

fn channel_summary_cmp(a: &ChannelSummary, b: &ChannelSummary) -> std::cmp::Ordering {
    a.position.cmp(&b.position).then(a.name.cmp(&b.name))
}

pub(crate) fn sort_channels_for_sidebar(channels: &mut Vec<ChannelSummary>) {
    channels.sort_by(channel_summary_cmp);

    let mut uncategorized = Vec::new();
    let mut categories = Vec::new();
    let mut children: HashMap<String, Vec<ChannelSummary>> = HashMap::new();

    for channel in channels.drain(..) {
        match channel.kind {
            ChannelKind::Category => categories.push(channel),
            _ if channel.parent_id.is_none() => uncategorized.push(channel),
            _ => {
                debug_assert!(channel.parent_id.is_some());
                let Some(parent_id) = channel.parent_id.clone() else {
                    continue;
                };
                children.entry(parent_id).or_default().push(channel);
            }
        }
    }

    categories.sort_by(channel_summary_cmp);
    uncategorized.sort_by(channel_summary_cmp);
    for child_group in children.values_mut() {
        child_group.sort_by(channel_summary_cmp);
    }

    channels.extend(uncategorized);
    for category in categories {
        let category_id = category.id.clone();
        channels.push(category);
        if let Some(group_children) = children.remove(&category_id) {
            channels.extend(group_children);
        }
    }
    for orphan_children in children.into_values() {
        channels.extend(orphan_children);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoadScope {
    StartupConnect,
    GuildBootstrap,
    ChannelList(String),
    History(String),
}

impl LoadScope {
    pub const fn display_priority(&self) -> u8 {
        match self {
            Self::History(_) => 4,
            Self::ChannelList(_) => 3,
            Self::GuildBootstrap => 2,
            Self::StartupConnect => 1,
        }
    }

    pub const fn status_label(&self) -> &'static str {
        match self {
            Self::StartupConnect => "Connecting",
            Self::GuildBootstrap => "Loading guilds",
            Self::ChannelList(_) => "Loading channels",
            Self::History(_) => "Refreshing history",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GuildMuteSettings {
    pub guild_id: String,
    pub muted: bool,
    pub channel_overrides: HashMap<String, ChannelMuteOverride>,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelMuteOverride {
    pub muted: bool,
}

#[derive(Debug, Clone)]
pub struct AttachmentSummary {
    #[expect(
        dead_code,
        reason = "reserved for attachment identity and future actions"
    )]
    pub id: String,
    pub filename: String,
    pub url: String,
    #[expect(dead_code, reason = "reserved for richer media cards")]
    pub content_type: Option<String>,
    #[expect(dead_code, reason = "reserved for responsive image sizing")]
    pub width: Option<u64>,
    #[expect(dead_code, reason = "reserved for responsive image sizing")]
    pub height: Option<u64>,
    pub is_image: bool,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: String,
    pub channel_id: String,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub content: String,
    pub attachments: Vec<AttachmentSummary>,
    pub timestamp: String,
    pub edited: bool,
    pub is_continuation: bool,
}

#[derive(Debug, Clone)]
pub struct ChannelDigest {
    pub channel_id: String,
    pub summary: String,
    pub todos: Vec<TodoItem>,
    pub generated_at: String,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub author: String,
    pub snippet: String,
    pub reason: String,
    pub message_id: String,
}
