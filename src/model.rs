#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Category,
    Text,
    #[cfg_attr(not(feature = "experimental-discord"), expect(dead_code))]
    Announcement,
}

impl ChannelKind {
    pub const fn is_selectable(self) -> bool {
        !matches!(self, Self::Category)
    }
}

#[derive(Debug, Clone)]
pub struct GuildSummary {
    pub id: String,
    pub name: String,
    pub unread: bool,
    pub unread_count: u32,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelSummary {
    pub id: String,
    #[expect(
        dead_code,
        reason = "used in live transport normalization and future guild rail views"
    )]
    pub guild_id: Option<String>,
    #[cfg_attr(not(feature = "experimental-discord"), expect(dead_code))]
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: ChannelKind,
    #[cfg_attr(not(feature = "experimental-discord"), expect(dead_code))]
    pub position: i32,
    pub unread: bool,
    pub unread_count: u32,
    /// Last message ID in this channel (from Discord), used for mark-all-read persistence.
    pub last_message_id: Option<String>,
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
