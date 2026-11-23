use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use super::{Member, PermissionOverwrite, Permissions};
use crate::{HttpClient, Message, User};

/// Represents a Discord channel (text, voice, DM, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ChannelType {
    GuildText = 0,
    DM = 1,
    GuildVoice = 2,
    GroupDM = 3,
    GuildCategory = 4,
    GuildAnnouncement = 5,
    AnnouncementThread = 10,
    PublicThread = 11,
    PrivateThread = 12,
    GuildStageVoice = 13,
    GuildDirectory = 14,
    GuildForum = 15,
    GuildMedia = 16,
    /// A channel type newer than this list. Without it one such channel
    /// fails the whole guild's deserialization.
    #[serde(other)]
    Unknown = 255,
}

#[cfg(test)]
mod channel_type_tests {
    use crate::model::ChannelType;

    #[test]
    fn a_newer_channel_type_deserializes_as_unknown() {
        let kinds: Vec<ChannelType> = serde_json::from_str("[0, 20, 16]").expect("channel types");
        assert_eq!(
            kinds,
            [
                ChannelType::GuildText,
                ChannelType::Unknown,
                ChannelType::GuildMedia
            ]
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    /// Unique ID of the channel
    pub id: String,

    /// Type of the channel (text, voice, DM, etc.)
    #[serde(rename = "type")]
    pub kind: ChannelType,

    /// Id of the guild (if applicable)
    pub guild_id: Option<String>,

    /// Position of the channel in the guild (if applicable)
    pub position: Option<i32>,

    /// Explicit permission overwrites for roles/users in this channel
    #[serde(default)]
    pub permission_overwrites: Vec<PermissionOverwrite>,

    /// Name of the channel (if applicable)
    pub name: Option<String>,

    /// Topic of the channel (if applicable)
    pub topic: Option<String>,

    /// Whether the channel is NSFW (if applicable)
    #[serde(default)]
    pub nsfw: bool,

    /// Id of the last message sent in the channel (if applicable)
    pub last_message_id: Option<String>,

    /// Bitrate (for voice channels)
    pub bitrate: Option<u64>,

    /// User limit (for voice channels)
    pub user_limit: Option<u64>,

    /// Rate limit per user (for text channels)
    pub rate_limit_per_user: Option<u64>,

    /// recipients (for DM channels)
    pub recipients: Option<Vec<User>>,

    /// Icon hash (for group DM channels)
    pub icon: Option<String>,

    /// Owner ID (for group DM channels)
    pub owner_id: Option<String>,

    /// Application ID (for group DM channels)
    pub application_id: Option<String>,

    /// Whether the channel is managed
    #[serde(default)]
    pub managed: bool,

    /// Channel's parent category ID (if applicable)
    pub parent_id: Option<String>,

    /// The channel's last pinned message ID (if applicable)
    pub last_pin_timestamp: Option<String>,

    /// The channel's rtc region (for voice channels)
    pub rtc_region: Option<String>,

    /// The channel's video quality mode (for voice channels)
    pub video_quality_mode: Option<u8>,

    /// The channel's message count (for threads)
    pub message_count: Option<u64>,

    /// The channel's member count (for threads)
    pub member_count: Option<u64>,

    /// Thread metdata
    pub thread_metadata: Option<ThreadMetadata>,

    /// Thread member object (for threads the current user has joined)
    pub member: Option<ThreadMember>,

    /// Default auto archive duration for threads in this channel (if applicable)
    pub default_auto_archive_duration: Option<u64>,

    /// Permissions (for threads)
    pub permissions: Option<Permissions>,

    /// Flags
    pub flags: Option<u64>,

    /// Total number of messages in the thread, even when messages are deleted (if applicable)
    pub total_messages: Option<u64>,

    /// Available tags in a guild forum channel
    pub available_tags: Option<Vec<ForumTag>>,

    /// Applied tags IDs in a thread in a guild forum channel
    pub applied_tags: Option<Vec<String>>,

    /// Default sort order type for a guild forum channel
    pub default_sort_order: Option<u8>,

    /// Default forum layout view for a guild forum channel
    pub default_forum_layout: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMember {
    /// The ID of the thread
    #[serde(rename = "id")]
    pub thread_id: String,

    /// The ID of the user
    pub user_id: String,

    /// The timestamp when the user joined the thread
    pub join_timestamp: String,

    /// The flags for the user in the thread
    pub flags: u64,

    /// Whether the user has muted the thread
    #[serde(default)]
    pub muted: bool,

    /// The member object for the user
    #[serde(default)]
    pub member: Option<Member>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMention {
    /// Unique ID of the channel
    pub id: String,

    /// Id of the guild the channel belongs to
    pub guild_id: String,

    /// Name of the channel
    pub name: String,

    /// Type of the channel (text, voice, DM, etc.)
    #[serde(rename = "type")]
    pub kind: ChannelType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumTag {
    /// The id of the tag
    pub id: Option<String>,

    /// The name of the tag
    pub name: String,

    /// Moderated (whether users can add this tag to their threads)
    #[serde(default)]
    pub moderated: bool,

    /// Custom emoji ID associated with the tag (if any)
    pub emoji_id: Option<String>,

    /// Emoji name associated with the tag (if any, used if emoji_id is null)
    pub emoji_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMetadata {
    /// Whether the thread is archived
    pub archived: bool,

    /// Timestamp when the thread was archived
    pub archive_timestamp: String,

    /// Whether the thread is locked
    pub locked: bool,

    /// Whether non-moderators can unarchive the thread
    pub invitable: Option<bool>,

    /// Create Timestamp of the thread (for threads created before 2022-01-09)
    pub create_timestamp: Option<String>,
}

impl Channel {
    /// Helper method to check if the channel is a DM or Group DM
    pub fn is_dm(&self) -> bool {
        matches!(self.kind, ChannelType::DM | ChannelType::GroupDM)
    }

    /// Helper method to get the mention string for the channel
    pub fn mention(&self) -> String {
        if self.is_dm() {
            format!("<@{}>", self.id)
        } else {
            format!("<#{}>", self.id)
        }
    }
    /// Get the guild for this channel (if applicable)
    ///
    /// # Example
    /// ```ignore
    /// use diself::{HttpClient, model::channel::Channel};
    ///
    /// async fn example(http: &HttpClient, channel: &Channel) {
    ///     if let Some(guild) = channel.guild(http).await {
    ///         println!("Channel is in guild: {}", guild.name);
    ///     } else {
    ///         println!("Channel is not in a guild");
    ///     }
    /// }
    /// ```
    pub async fn guild(&self, http: &HttpClient) -> Option<crate::model::guild::Guild> {
        if let Some(guild_id) = &self.guild_id {
            let url = crate::http::api_url(&format!("/guilds/{}", guild_id));
            if let Ok(response) = http.get(&url).await {
                match serde_json::from_value(response) {
                    Ok(guild) => return Some(guild),
                    Err(e) => {
                        eprintln!("Failed to deserialize guild: {}", e);
                        return None;
                    }
                }
            }
            None
        } else {
            None
        }
    }
    /// Sends a message to this channel
    pub async fn send(
        &self,
        http: &HttpClient,
        content: impl Into<String>,
    ) -> Result<Message, crate::error::Error> {
        // Sending a message always goes through the channel message endpoint,
        // including DM channels.
        let url = crate::http::api_url(&format!("/channels/{}/messages", self.id));
        let body = serde_json::json!({
            "content": content.into()
        });

        let response = http.post(&url, body).await?;
        let message: Message = serde_json::from_value(response)?;
        Ok(message)
    }

    /// Fetches messages from this channel. (`GET /channels/{channel_id}/messages`) SEE: <https://docs.discord.food/resources/message#get-messages>
    /// # Params
    /// - around?: Snowflake - Get messages around this message ID
    /// - before?: Snowflake - Get messages before this message ID
    /// - after?: Snowflake - Get messages after this message ID
    /// - limit?: number - Max number of messages to return (1-100, default 50)
    pub async fn messages(
        &self,
        http: &HttpClient,
        around: Option<String>,
        before: Option<String>,
        after: Option<String>,
        limit: Option<u8>,
    ) -> Result<Vec<Message>, crate::error::Error> {
        let mut url = crate::http::api_url(&format!("/channels/{}/messages", self.id));
        let mut query_params = vec![];

        if let Some(around) = around {
            query_params.push(("around", around));
        }
        if let Some(before) = before {
            query_params.push(("before", before));
        }
        if let Some(after) = after {
            query_params.push(("after", after));
        }
        if let Some(limit) = limit {
            query_params.push(("limit", limit.to_string()));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(
                &query_params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }

        let response = http.get(&url).await?;
        let messages: Vec<Message> = serde_json::from_value(response)?;
        Ok(messages)
    }

    /// Fetches a single message by ID from this channel. (`GET /channels/{channel_id}/messages/{message_id}`) SEE: <https://docs.discord.food/resources/message#get-message>
    pub async fn get_message(
        &self,
        http: &HttpClient,
        message_id: impl AsRef<str>,
    ) -> Result<Message, crate::error::Error> {
        let url = crate::http::api_url(&format!(
            "/channels/{}/messages/{}",
            self.id,
            message_id.as_ref()
        ));
        let response = http.get(&url).await?;
        let message: Message = serde_json::from_value(response)?;
        Ok(message)
    }
}
