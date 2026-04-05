#![cfg(feature = "experimental-discord")]

use std::sync::Arc;

use async_trait::async_trait;
use diself::prelude::*;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::action::Action;
use crate::model::{
    AttachmentSummary, ChannelKind, ChannelSummary, DIRECT_MESSAGES_GUILD_ID, GuildMuteSettings,
    GuildSummary, MessageRow, sort_channels_for_sidebar,
};

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
    async fn on_gateway_payload(&self, _ctx: &Context, payload: &serde_json::Value) {
        let is_dispatch = payload.get("op").and_then(serde_json::Value::as_u64) == Some(0);
        let is_settings_update = payload.get("t").and_then(serde_json::Value::as_str)
            == Some("USER_GUILD_SETTINGS_UPDATE");
        if !is_dispatch || !is_settings_update {
            return;
        }

        let Some(data) = payload.get("d") else {
            return;
        };
        self.send(Action::GuildMuteSettingsUpdated(
            parse_user_guild_mute_setting(data),
        ));
    }

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
        let guild_mute_settings = parse_user_guild_mute_settings(&data);

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
            guild_mute_settings,
        });
    }

    async fn on_message_create(&self, ctx: &Context, message: Message) {
        let channel_hint = ctx
            .cache
            .channel(&message.channel_id)
            .and_then(|channel| channel_to_summary(&channel));
        self.send(Action::MessageAppended {
            message: message_to_row(&message, false),
            channel_hint,
        });
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
                cache_channels: true,
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
    let author =
        preferred_user_name(&msg.author.username, msg.author.global_name.as_deref()).to_string();
    let content = render_message_content(
        &msg.content,
        msg.mentions.iter().map(|mentioned_user| {
            (
                mentioned_user.id.as_str(),
                preferred_user_name(
                    &mentioned_user.username,
                    mentioned_user.global_name.as_deref(),
                ),
            )
        }),
        msg.message_reference.is_some(),
    );

    MessageRow {
        id: msg.id.clone(),
        channel_id: msg.channel_id.clone(),
        author,
        author_avatar_url: user_avatar_url(&msg.author.id, msg.author.avatar.as_deref()),
        content,
        attachments: msg
            .attachments
            .iter()
            .map(|attachment| {
                attachment_summary(AttachmentSummaryInput {
                    id: attachment.id.clone(),
                    filename: attachment.filename.clone(),
                    url: attachment.proxy_url.clone(),
                    content_type: attachment.content_type.clone(),
                    width: attachment.width,
                    height: attachment.height,
                })
            })
            .collect(),
        timestamp,
        edited: msg.edited_timestamp.is_some(),
        is_continuation,
    }
}

fn preferred_user_name<'a>(username: &'a str, global_name: Option<&'a str>) -> &'a str {
    global_name.unwrap_or(username)
}

fn render_message_content<'a, I>(content: &str, mentions: I, is_reply: bool) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut rendered = content.to_string();
    for (user_id, display_name) in mentions {
        let mention_tag = format!("<@{user_id}>");
        rendered = rendered.replace(&mention_tag, &format!("@{display_name}"));
        let nick_tag = format!("<@!{user_id}>");
        rendered = rendered.replace(&nick_tag, &format!("@{display_name}"));
    }

    if is_reply {
        return format!("↩ {rendered}");
    }

    rendered
}

struct AttachmentSummaryInput {
    id: String,
    filename: String,
    url: String,
    content_type: Option<String>,
    width: Option<u64>,
    height: Option<u64>,
}

fn attachment_summary(input: AttachmentSummaryInput) -> AttachmentSummary {
    let AttachmentSummaryInput {
        id,
        filename,
        url,
        content_type,
        width,
        height,
    } = input;
    let is_image = content_type
        .as_deref()
        .is_some_and(|content_type| content_type.starts_with("image/"));

    AttachmentSummary {
        id,
        filename,
        url,
        content_type,
        width,
        height,
        is_image,
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
                muted: false,
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
    let is_direct_message = matches!(ch.kind, ChannelType::DM | ChannelType::GroupDM);
    let kind = match ch.kind {
        ChannelType::GuildText
        | ChannelType::AnnouncementThread
        | ChannelType::PublicThread
        | ChannelType::PrivateThread => ChannelKind::Text,
        ChannelType::GuildAnnouncement => ChannelKind::Announcement,
        ChannelType::GuildCategory => ChannelKind::Category,
        ChannelType::DM | ChannelType::GroupDM => ChannelKind::DirectMessage,
        _ => return None,
    };
    Some(ChannelSummary {
        id: ch.id.clone(),
        guild_id: ch
            .guild_id
            .clone()
            .or_else(|| is_direct_message.then(|| DIRECT_MESSAGES_GUILD_ID.to_string())),
        parent_id: ch.parent_id.clone(),
        name: channel_name(ch),
        kind,
        position: ch.position.unwrap_or_default(),
        muted: false,
        unread: false,
        unread_count: 0,
        last_message_id: ch.last_message_id.clone(),
    })
}

pub fn channels_to_summaries(channels: &[diself::Channel]) -> Vec<ChannelSummary> {
    let mut summaries: Vec<_> = channels.iter().filter_map(channel_to_summary).collect();
    sort_channels_for_sidebar(&mut summaries);
    summaries
}

pub fn apply_read_state_from_cache(channels: &mut [ChannelSummary], cache: &diself::Cache) {
    for channel in channels {
        let Some(read_state) = cache.read_state(&channel.id) else {
            continue;
        };

        let last_acked = read_state
            .last_acked_id
            .as_deref()
            .and_then(|id| id.parse::<u64>().ok())
            .unwrap_or_default();
        let last_message = channel
            .last_message_id
            .as_deref()
            .or(read_state.last_message_id.as_deref())
            .and_then(|id| id.parse::<u64>().ok())
            .unwrap_or_default();

        if last_message > last_acked {
            channel.unread = true;
            channel.unread_count = read_state
                .mention_count
                .or(read_state.badge_count)
                .and_then(|count| u32::try_from(count).ok())
                .unwrap_or_default();
            tracing::debug!(
                channel_id = %channel.id,
                channel_name = %channel.name,
                kind = ?channel.kind,
                guild_id = ?channel.guild_id,
                last_acked,
                last_message,
                mention_count = ?read_state.mention_count,
                badge_count = ?read_state.badge_count,
                unread_count = channel.unread_count,
                "cache read state marked channel unread"
            );
        }

        if channel.last_message_id.is_none() {
            channel
                .last_message_id
                .clone_from(&read_state.last_message_id);
            tracing::debug!(
                channel_id = %channel.id,
                channel_name = %channel.name,
                read_state_last_message_id = ?read_state.last_message_id,
                "filled channel last_message_id from cache read state"
            );
        }
    }
}

fn parse_user_guild_mute_settings(
    data: &serde_json::Value,
) -> std::collections::HashMap<String, GuildMuteSettings> {
    user_guild_settings_entries(data)
        .into_iter()
        .map(parse_user_guild_mute_setting)
        .map(|settings| {
            let guild_id = settings.guild_id.clone();
            (guild_id, settings)
        })
        .collect()
}

fn parse_user_guild_mute_setting(data: &serde_json::Value) -> GuildMuteSettings {
    let mut settings = GuildMuteSettings {
        guild_id: normalize_user_guild_settings_id(data),
        muted: data
            .get("muted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        channel_overrides: std::collections::HashMap::new(),
    };

    let Some(overrides) = data.get("channel_overrides") else {
        return settings;
    };

    match overrides {
        serde_json::Value::Array(entries) => {
            for override_entry in entries {
                let Some(channel_id) = override_entry
                    .get("channel_id")
                    .and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                settings.channel_overrides.insert(
                    channel_id.to_string(),
                    parse_channel_mute_override(override_entry),
                );
            }
        }
        serde_json::Value::Object(entries) => {
            for (channel_id, override_entry) in entries {
                settings.channel_overrides.insert(
                    channel_id.clone(),
                    parse_channel_mute_override(override_entry),
                );
            }
        }
        _ => {}
    }

    settings
}

fn normalize_user_guild_settings_id(data: &serde_json::Value) -> String {
    match data.get("guild_id").and_then(serde_json::Value::as_str) {
        Some("@me") | None => DIRECT_MESSAGES_GUILD_ID.to_string(),
        Some(guild_id) => guild_id.to_string(),
    }
}

fn parse_channel_mute_override(
    override_entry: &serde_json::Value,
) -> crate::model::ChannelMuteOverride {
    crate::model::ChannelMuteOverride {
        muted: override_entry
            .get("muted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

fn user_guild_settings_entries(data: &serde_json::Value) -> Vec<&serde_json::Value> {
    data.get("user_guild_settings")
        .and_then(|settings| {
            settings
                .get("entries")
                .and_then(serde_json::Value::as_array)
                .or_else(|| settings.as_array())
        })
        .map_or_else(Vec::new, |entries| entries.iter().collect())
}

pub fn guild_to_summary(id: &str, name: &str, icon_hash: Option<&str>) -> GuildSummary {
    GuildSummary {
        id: id.into(),
        name: name.into(),
        muted: false,
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

fn channel_name(channel: &diself::Channel) -> String {
    if let Some(name) = channel.name.as_deref().filter(|name| !name.is_empty()) {
        return name.to_string();
    }

    if let Some(recipients) = channel
        .recipients
        .as_ref()
        .filter(|users| !users.is_empty())
    {
        return recipients
            .iter()
            .map(|user| {
                user.global_name
                    .as_deref()
                    .unwrap_or(&user.username)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(", ");
    }

    "unnamed".to_string()
}

/// Convert messages from diself (newest-first from REST) to chronological order.
#[derive(Debug, Deserialize)]
struct HistoryMessagePayload {
    id: String,
    #[serde(default)]
    channel_id: String,
    author: HistoryUserPayload,
    #[serde(default)]
    content: String,
    timestamp: String,
    #[serde(default)]
    edited_timestamp: Option<String>,
    #[serde(default)]
    mentions: Vec<HistoryUserPayload>,
    #[serde(default)]
    attachments: Vec<HistoryAttachmentPayload>,
    #[serde(default)]
    message_reference: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct HistoryUserPayload {
    id: String,
    username: String,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryAttachmentPayload {
    id: String,
    filename: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    proxy_url: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    width: Option<u64>,
    #[serde(default)]
    height: Option<u64>,
}

pub fn history_rows_from_value(
    requested_channel_id: &str,
    value: serde_json::Value,
) -> serde_json::Result<Vec<MessageRow>> {
    let messages: Vec<HistoryMessagePayload> = serde_json::from_value(value)?;
    let mut rows: Vec<MessageRow> = Vec::with_capacity(messages.len());

    for msg in messages.into_iter().rev() {
        let author = preferred_user_name(&msg.author.username, msg.author.global_name.as_deref())
            .to_string();
        let is_continuation = rows.last().is_some_and(|prev| prev.author == author);

        let content = render_message_content(
            &msg.content,
            msg.mentions.iter().map(|mentioned_user| {
                (
                    mentioned_user.id.as_str(),
                    preferred_user_name(
                        &mentioned_user.username,
                        mentioned_user.global_name.as_deref(),
                    ),
                )
            }),
            msg.message_reference.is_some(),
        );

        let attachments = msg
            .attachments
            .into_iter()
            .map(|attachment| {
                let url = if attachment.proxy_url.is_empty() {
                    attachment.url
                } else {
                    attachment.proxy_url
                };
                attachment_summary(AttachmentSummaryInput {
                    id: attachment.id,
                    filename: attachment.filename,
                    url,
                    content_type: attachment.content_type,
                    width: attachment.width,
                    height: attachment.height,
                })
            })
            .collect();

        rows.push(MessageRow {
            id: msg.id,
            channel_id: if msg.channel_id.is_empty() {
                requested_channel_id.to_string()
            } else {
                msg.channel_id
            },
            author,
            author_avatar_url: user_avatar_url(&msg.author.id, msg.author.avatar.as_deref()),
            content,
            attachments,
            timestamp: parse_discord_timestamp(&msg.timestamp),
            edited: msg.edited_timestamp.is_some(),
            is_continuation,
        });
    }

    Ok(rows)
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
    fn history_rows_ignore_sticker_items_without_description() {
        let payload = serde_json::json!([
            {
                "id": "m1",
                "channel_id": "dm1",
                "author": {
                    "id": "u1",
                    "username": "alice",
                    "global_name": "Alice",
                    "avatar": null
                },
                "content": "hi <@u2>",
                "timestamp": "2026-04-02T10:15:30.123456+00:00",
                "edited_timestamp": null,
                "mentions": [
                    {
                        "id": "u2",
                        "username": "bob",
                        "global_name": "Bob",
                        "avatar": null
                    }
                ],
                "attachments": [],
                "message_reference": null,
                "sticker_items": [
                    {
                        "id": "s1",
                        "name": "wave",
                        "format_type": 1
                    }
                ]
            }
        ]);

        let rows_result = history_rows_from_value("dm1", payload);
        assert!(
            rows_result.is_ok(),
            "history payload should parse: {rows_result:?}"
        );
        let rows = rows_result.unwrap_or_default();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].channel_id, "dm1");
        assert_eq!(rows[0].content, "hi @Bob");
    }

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
