#![cfg(feature = "experimental-discord")]

use std::sync::Arc;

use async_trait::async_trait;
use diself::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::action::Action;
use crate::model::{AttachmentSummary, ChannelKind, ChannelSummary, GuildSummary, MessageRow};

pub struct DiscordBridge {
    tx: mpsc::Sender<Action>,
}

impl DiscordBridge {
    pub fn new(tx: mpsc::Sender<Action>) -> Self {
        Self { tx }
    }

    fn send(&self, action: Action) {
        match self.tx.try_send(action) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!("action channel full, dropping event");
            }
            Err(TrySendError::Closed(_)) => {
                tracing::warn!("action channel closed, dropping event");
            }
        }
    }
}

#[async_trait]
impl EventHandler for DiscordBridge {
    async fn on_ready(&self, _ctx: &Context, user: User) {
        let username = user
            .global_name
            .as_deref()
            .unwrap_or(&user.username)
            .to_string();
        tracing::info!("connected as {username}");
        self.send(Action::TransportConnected { username });
    }

    async fn on_ready_event(&self, _ctx: &Context, data: serde_json::Value) {
        let Some(guilds_json) = data.get("guilds").and_then(serde_json::Value::as_array) else {
            return;
        };

        tracing::info!("READY contains {} guilds", guilds_json.len());

        let mut guild_summaries = Vec::new();
        let mut all_channels = std::collections::HashMap::new();

        for guild in guilds_json {
            let props = guild.get("properties").unwrap_or(guild);
            let id = props
                .get("id")
                .or_else(|| guild.get("id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let name = props
                .get("name")
                .or_else(|| guild.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unknown");

            if id.is_empty() {
                continue;
            }

            let icon = props
                .get("icon")
                .or_else(|| guild.get("icon"))
                .and_then(serde_json::Value::as_str);
            guild_summaries.push(guild_to_summary(id, name, icon));

            let channels: Vec<ChannelSummary> = guild
                .get("channels")
                .and_then(serde_json::Value::as_array)
                .map(|arr| parse_channels_from_json(arr, id))
                .unwrap_or_default();

            if !channels.is_empty() {
                all_channels.insert(id.to_string(), channels);
            }
        }

        let unread_channels = parse_read_state(&data);

        // Apply unread state to channels
        for channels in all_channels.values_mut() {
            for ch in channels.iter_mut() {
                if let Some(mentions) = unread_channels.get(&ch.id) {
                    ch.unread = true;
                    ch.unread_count = *mentions;
                }
            }
        }

        // Also mark guilds as unread if any of their channels are unread
        for guild in &mut guild_summaries {
            if let Some(channels) = all_channels.get(&guild.id) {
                let total_unread: u32 = channels
                    .iter()
                    .filter(|ch| ch.unread)
                    .map(|ch| ch.unread_count.max(1))
                    .sum();
                if total_unread > 0 {
                    guild.unread = true;
                    guild.unread_count = total_unread;
                }
            }
        }

        self.send(Action::ReadyData {
            guilds: guild_summaries,
            guild_channels: all_channels,
        });
    }

    async fn on_message_create(&self, _ctx: &Context, message: Message) {
        self.send(Action::MessageAppended(message_to_row(&message, false)));
    }

    async fn on_message_update(&self, _ctx: &Context, message: Message) {
        let mut row = message_to_row(&message, false);
        row.edited = true;
        self.send(Action::MessagePatched(row));
    }

    async fn on_message_delete(&self, _ctx: &Context, channel_id: String, message_id: String) {
        self.send(Action::MessageRemoved {
            channel_id,
            message_id,
        });
    }

    async fn on_guild_create(&self, _ctx: &Context, data: serde_json::Value) {
        let props = data.get("properties").unwrap_or(&data);
        let id = props
            .get("id")
            .or_else(|| data.get("id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let name = props
            .get("name")
            .or_else(|| data.get("name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown");

        if id.is_empty() {
            return;
        }

        tracing::info!("guild_create: {name} ({id})");

        let channels: Vec<ChannelSummary> = data
            .get("channels")
            .and_then(serde_json::Value::as_array)
            .map(|arr| parse_channels_from_json(arr, id))
            .unwrap_or_default();

        self.send(Action::GuildAvailable {
            guild: guild_to_summary(
                id,
                name,
                props
                    .get("icon")
                    .or_else(|| data.get("icon"))
                    .and_then(serde_json::Value::as_str),
            ),
            channels,
        });
    }
}

/// Start the diself client and return a handle for shutdown.
pub fn connect(
    token: String,
    tx: mpsc::Sender<Action>,
) -> color_eyre::eyre::Result<(Arc<diself::Client>, tokio::task::JoinHandle<()>)> {
    let err_tx = tx.clone();
    let bridge = DiscordBridge::new(tx);

    let client = Arc::new(
        diself::Client::builder(token, bridge)
            .map_err(|e| color_eyre::eyre::eyre!("failed to build discord client: {e}"))?
            .with_cache_config(CacheConfig {
                cache_users: false,
                cache_channels: false,
                cache_guilds: false,
                cache_relationships: false,
            })
            .build(),
    );

    let runner = Arc::clone(&client);
    let handle = tokio::spawn(async move {
        if let Err(e) = runner.start().await {
            tracing::error!("discord client error: {e}");
            let _ = err_tx
                .send(Action::TransportDisconnected(format!("gateway error: {e}")))
                .await;
        }
    });

    Ok((client, handle))
}

pub fn message_to_row(msg: &Message, is_continuation: bool) -> MessageRow {
    let timestamp = parse_discord_timestamp(&msg.timestamp);
    let author = msg
        .author
        .global_name
        .as_deref()
        .unwrap_or(&msg.author.username)
        .to_string();

    // Resolve <@userid> mentions to display names
    let mut content = msg.content.clone();
    for mentioned_user in &msg.mentions {
        let mention_tag = format!("<@{}>", mentioned_user.id);
        let display_name = mentioned_user
            .global_name
            .as_deref()
            .unwrap_or(&mentioned_user.username);
        content = content.replace(&mention_tag, &format!("@{display_name}"));
        // Also handle the nickname mention format <@!userid>
        let nick_tag = format!("<@!{}>", mentioned_user.id);
        content = content.replace(&nick_tag, &format!("@{display_name}"));
    }

    // Prefix reply indicator if this is a reply
    if msg.message_reference.is_some() {
        content = format!("↩ {content}");
    }

    MessageRow {
        id: msg.id.clone(),
        channel_id: msg.channel_id.clone(),
        author,
        author_avatar_url: user_avatar_url(&msg.author.id, msg.author.avatar.as_deref()),
        content,
        attachments: msg
            .attachments
            .iter()
            .map(|attachment| AttachmentSummary {
                id: attachment.id.clone(),
                filename: attachment.filename.clone(),
                url: attachment.proxy_url.clone(),
                content_type: attachment.content_type.clone(),
                width: attachment.width,
                height: attachment.height,
                is_image: attachment
                    .content_type
                    .as_deref()
                    .is_some_and(|content_type| content_type.starts_with("image/")),
            })
            .collect(),
        timestamp,
        edited: msg.edited_timestamp.is_some(),
        is_continuation,
    }
}

fn parse_read_state(data: &serde_json::Value) -> std::collections::HashMap<String, u32> {
    let mut unread = std::collections::HashMap::new();
    let Some(entries) = data.get("read_state").and_then(|v| {
        v.get("entries")
            .and_then(serde_json::Value::as_array)
            .or_else(|| v.as_array())
    }) else {
        return unread;
    };

    for rs in entries {
        let channel_id = rs
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if channel_id.is_empty() {
            continue;
        }
        let last_acked: u64 = rs
            .get("last_acked_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let last_msg: u64 = rs
            .get("last_message_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mention_count = rs
            .get("mention_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        if last_msg > last_acked {
            unread.insert(
                channel_id.to_string(),
                u32::try_from(mention_count).unwrap_or(u32::MAX),
            );
        }
    }

    tracing::info!("read_state: {} channels with unread messages", unread.len());
    unread
}

fn parse_channels_from_json(arr: &[serde_json::Value], guild_id: &str) -> Vec<ChannelSummary> {
    let mut channels: Vec<ChannelSummary> = arr
        .iter()
        .filter_map(|ch| {
            let ch_type = ch
                .get("type")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(99);
            let kind = match ch_type {
                0 => ChannelKind::Text,
                4 => ChannelKind::Category,
                5 => ChannelKind::Announcement,
                _ => return None,
            };
            Some(ChannelSummary {
                id: ch.get("id").and_then(serde_json::Value::as_str)?.into(),
                guild_id: Some(guild_id.into()),
                parent_id: ch
                    .get("parent_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                name: ch
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unnamed")
                    .into(),
                kind,
                position: ch
                    .get("position")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or_default(),
                unread: false,
                unread_count: 0,
                last_message_id: ch
                    .get("last_message_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect();
    sort_channels_for_sidebar(&mut channels);
    channels
}

pub fn channel_to_summary(ch: &diself::Channel) -> Option<ChannelSummary> {
    use diself::model::ChannelType;
    let kind = match ch.kind {
        ChannelType::GuildText => ChannelKind::Text,
        ChannelType::GuildAnnouncement => ChannelKind::Announcement,
        ChannelType::GuildCategory => ChannelKind::Category,
        _ => return None,
    };
    Some(ChannelSummary {
        id: ch.id.clone(),
        guild_id: ch.guild_id.clone(),
        parent_id: ch.parent_id.clone(),
        name: ch.name.clone().unwrap_or_else(|| "unnamed".into()),
        kind,
        position: ch.position.unwrap_or_default(),
        unread: false,
        unread_count: 0,
        last_message_id: ch.last_message_id.clone(),
    })
}

pub fn guild_to_summary(id: &str, name: &str, icon_hash: Option<&str>) -> GuildSummary {
    GuildSummary {
        id: id.into(),
        name: name.into(),
        unread: false,
        unread_count: 0,
        avatar_url: guild_icon_url(id, icon_hash),
    }
}

fn guild_icon_url(guild_id: &str, icon_hash: Option<&str>) -> Option<String> {
    let hash = icon_hash?;
    Some(format!(
        "https://cdn.discordapp.com/icons/{guild_id}/{hash}.png?size=64"
    ))
}

fn user_avatar_url(user_id: &str, avatar_hash: Option<&str>) -> Option<String> {
    let hash = avatar_hash?;
    Some(format!(
        "https://cdn.discordapp.com/avatars/{user_id}/{hash}.png?size=64"
    ))
}

/// Convert messages from diself (newest-first from REST) to chronological order.
pub fn messages_to_rows(messages: &[Message]) -> Vec<MessageRow> {
    let mut rows: Vec<MessageRow> = Vec::with_capacity(messages.len());
    for msg in messages.iter().rev() {
        let author_name = msg
            .author
            .global_name
            .as_deref()
            .unwrap_or(&msg.author.username);
        let is_continuation = rows.last().is_some_and(|prev| prev.author == author_name);
        rows.push(message_to_row(msg, is_continuation));
    }
    rows
}

fn sort_channels_for_sidebar(channels: &mut Vec<ChannelSummary>) {
    channels.sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));

    let mut uncategorized = Vec::new();
    let mut categories = Vec::new();
    let mut children: std::collections::HashMap<String, Vec<ChannelSummary>> =
        std::collections::HashMap::new();

    for channel in channels.drain(..) {
        match channel.kind {
            ChannelKind::Category => categories.push(channel),
            _ if channel.parent_id.is_none() => uncategorized.push(channel),
            _ => {
                if let Some(parent_id) = channel.parent_id.clone() {
                    children.entry(parent_id).or_default().push(channel);
                }
            }
        }
    }

    categories.sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));
    uncategorized.sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));
    for child_group in children.values_mut() {
        child_group.sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));
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

fn parse_discord_timestamp(ts: &str) -> String {
    // Parse ISO-8601 UTC timestamp and convert to local time
    if let Ok(utc) = chrono::DateTime::parse_from_rfc3339(ts) {
        return utc
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string();
    }
    // Fallback: Discord sometimes sends non-RFC3339 timestamps
    // Try to extract HH:MM from after the T separator
    ts.split('T')
        .nth(1)
        .and_then(|time_part| time_part.get(..5))
        .map_or_else(|| ts.to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timestamp_iso_produces_hhmm() {
        let result = parse_discord_timestamp("2026-04-02T10:15:30.123456+00:00");
        // Result is local time HH:MM — exact value depends on timezone
        assert_eq!(result.len(), 5, "should produce HH:MM format");
        assert_eq!(&result[2..3], ":", "should have colon separator");
    }

    #[test]
    fn parse_timestamp_short() {
        assert_eq!(parse_discord_timestamp("2026-04-02T10:15"), "10:15");
    }

    #[test]
    fn parse_timestamp_fallback() {
        assert_eq!(
            parse_discord_timestamp("not a timestamp"),
            "not a timestamp"
        );
    }
}
