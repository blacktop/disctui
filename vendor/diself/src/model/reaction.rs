use super::User;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    /// Count of this reaction
    pub count: u64,

    /// Whether the current user has reacted with this emoji
    #[serde(default)]
    pub me: bool,

    /// The emoji itself
    pub emoji: Emoji,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Emoji {
    /// Unique ID of the emoji (if custom)
    pub id: Option<String>,

    /// Name of the emoji (can be null only in reaction emoji objects)
    pub name: Option<String>,

    /// Roles allowed to use this emoji
    #[serde(default)]
    pub roles: Vec<String>,

    /// User that created this emoji
    pub user: Option<User>,

    /// Whether this emoji must be wrapped in colons (e.g., <:emoji_name:emoji_id>)
    #[serde(default)]
    pub require_colons: bool,

    /// Whether this emoji is managed by an integration
    #[serde(default)]
    pub managed: bool,

    /// Whether this emoji is animated
    #[serde(default)]
    pub animated: bool,

    /// Whether this emoji is available
    #[serde(default)]
    pub available: bool,
}
