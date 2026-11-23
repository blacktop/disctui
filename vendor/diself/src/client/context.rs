use crate::cache::Cache;
use crate::client::{
    ChannelsManager, CollectorHub, CollectorOptions, GuildsManager, MessageCollector,
    ReactionCollectEvent, ReactionCollector, RelationshipsManager, UsersManager,
};
use crate::error::Result;
use crate::http::HttpClient;
use crate::model::{Channel, CreateMessage, Message, User};
use serde_json::json;
use std::path::Path;

/// Context passed to event handlers.
/// Contains references to useful clients and data.
#[derive(Clone)]
pub struct Context {
    /// HTTP client for making API requests
    pub http: HttpClient,
    /// The current user (bot)
    pub user: User,
    /// Cache for Discord entities
    pub cache: Cache,
    /// Users API manager
    pub users: UsersManager,
    /// Guilds API manager
    pub guilds: GuildsManager,
    /// Relationships API manager
    pub relationships: RelationshipsManager,
    /// Channels API Manager
    pub channels: ChannelsManager,
    /// Collector hub for message/reaction collectors
    pub collectors: CollectorHub,
}

impl Context {
    /// Creates a new context with a provided user
    pub fn new(http: HttpClient, user: User, cache: Cache) -> Self {
        // Cache the current user
        cache.set_current_user(user.clone());
        Self {
            http,
            user,
            cache,
            users: UsersManager,
            guilds: GuildsManager,
            relationships: RelationshipsManager,
            channels: ChannelsManager,
            collectors: CollectorHub::new(),
        }
    }

    /// Creates a context by fetching the current user from Discord API
    pub async fn create(http: HttpClient, cache: Cache) -> Result<Self> {
        let url = crate::http::api_url("/users/@me");
        let response = http.get(&url).await?;
        let user: User = serde_json::from_value(response)?;
        cache.set_current_user(user.clone());
        Ok(Self {
            http,
            user,
            cache,
            users: UsersManager,
            guilds: GuildsManager,
            relationships: RelationshipsManager,
            channels: ChannelsManager,
            collectors: CollectorHub::new(),
        })
    }

    /// Creates a message collector for MESSAGE_CREATE events.
    ///
    /// # Example
    /// ```ignore
    /// use diself::{CollectorOptions, Context};
    /// use std::time::Duration;
    ///
    /// async fn example(ctx: &Context) {
    ///     let mut collector = ctx.message_collector(
    ///         CollectorOptions {
    ///             time: Some(Duration::from_secs(20)),
    ///             max: Some(5),
    ///         },
    ///         |m| m.content.contains("hello"),
    ///     );
    ///
    ///     if let Some(msg) = collector.next().await {
    ///         println!("First match: {}", msg.content);
    ///     }
    /// }
    /// ```
    pub fn message_collector<F>(&self, options: CollectorOptions, filter: F) -> MessageCollector
    where
        F: Fn(&Message) -> bool + Send + Sync + 'static,
    {
        self.collectors.message_collector(options, filter)
    }

    /// Creates a reaction collector for reaction add/remove events.
    ///
    /// # Example
    /// ```ignore
    /// use diself::{CollectorOptions, Context, ReactionEventType};
    /// use std::time::Duration;
    ///
    /// async fn example(ctx: &Context, target_message_id: &str) {
    ///     let mut collector = ctx.reaction_collector(
    ///         CollectorOptions {
    ///             time: Some(Duration::from_secs(30)),
    ///             max: Some(1),
    ///         },
    ///         move |r| r.message_id == target_message_id && r.kind == ReactionEventType::Add,
    ///     );
    ///
    ///     let _ = collector.next().await;
    /// }
    /// ```
    pub fn reaction_collector<F>(&self, options: CollectorOptions, filter: F) -> ReactionCollector
    where
        F: Fn(&ReactionCollectEvent) -> bool + Send + Sync + 'static,
    {
        self.collectors.reaction_collector(options, filter)
    }

    /// Gets the current user reference
    pub fn current_user(&self) -> &User {
        &self.user
    }

    // ==================== Image Data Helpers ====================

    /// Converts image bytes to Discord Data URI format
    ///
    /// # Example
    /// ```ignore
    /// let image_data = std::fs::read("avatar.png")?;
    /// let data_uri = Context::image_to_data_uri(&image_data, "image/png");
    /// ```
    pub fn image_to_data_uri(image_bytes: &[u8], content_type: &str) -> String {
        let base64_encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, image_bytes);
        format!("data:{};base64,{}", content_type, base64_encoded)
    }

    /// Reads an image file and converts it to Data URI
    ///
    /// Automatically detects the content type based on file extension
    ///
    /// # Example
    /// ```ignore
    /// let data_uri = ctx.read_image_as_data_uri("avatar.png").await?;
    /// ```
    pub async fn read_image_as_data_uri(&self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let image_bytes = tokio::fs::read(path).await?;

        // Detect content type from extension
        let content_type = match path.extension().and_then(|s| s.to_str()) {
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("png") => "image/png",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/png", // Default
        };

        Ok(Self::image_to_data_uri(&image_bytes, content_type))
    }

    /// Downloads an image from a URL and converts it to Data URI
    ///
    /// # Example
    /// ```ignore
    /// let data_uri = ctx.download_image_as_data_uri("https://i.imgur.com/avatar.png").await?;
    /// ```
    pub async fn download_image_as_data_uri(&self, url: impl AsRef<str>) -> Result<String> {
        let url = url.as_ref();

        // Download the image
        let response = reqwest::get(url).await?;

        // Get content type from headers
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/png")
            .to_string();

        // Get image bytes
        let image_bytes = response.bytes().await?;

        // Convert to data URI
        Ok(Self::image_to_data_uri(&image_bytes, &content_type))
    }

    // ==================== User Methods ====================

    /// Refreshes and returns the current user data
    pub async fn refresh_current_user(&mut self) -> Result<User> {
        let url = crate::http::api_url("/users/@me");
        let response = self.http.get(&url).await?;
        let user: User = serde_json::from_value(response)?;
        self.user = user.clone();
        Ok(user)
    }

    /// Gets a user by ID
    pub async fn get_user(&self, user_id: impl AsRef<str>) -> Result<User> {
        let url = crate::http::api_url(&format!("/users/{}", user_id.as_ref()));
        let response = self.http.get(&url).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Updates the current user's username
    pub async fn update_username(&self, new_username: impl Into<String>) -> Result<User> {
        let url = crate::http::api_url("/users/@me");
        let body = json!({
            "username": new_username.into()
        });
        let response = self.http.patch(&url, body).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Updates the current user's avatar from a Data URI
    ///
    /// # Example
    /// ```ignore
    /// let data_uri = "data:image/png;base64,iVBORw0KG...";
    /// ctx.update_avatar_from_data_uri(data_uri).await?;
    /// ```
    pub async fn update_avatar_from_data_uri(&self, data_uri: impl Into<String>) -> Result<User> {
        let url = crate::http::api_url("/users/@me");
        let body = json!({
            "avatar": data_uri.into()
        });
        let response = self.http.patch(&url, body).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Updates the current user's avatar from a file path
    ///
    /// # Example
    /// ```ignore
    /// ctx.update_avatar("avatar.png").await?;
    /// ```
    pub async fn update_avatar(&self, image_path: impl AsRef<Path>) -> Result<User> {
        let data_uri = self.read_image_as_data_uri(image_path).await?;
        self.update_avatar_from_data_uri(data_uri).await
    }

    /// Updates the current user's avatar from raw bytes
    ///
    /// # Example
    /// ```ignore
    /// let image_bytes = std::fs::read("avatar.png")?;
    /// ctx.update_avatar_from_bytes(&image_bytes, "image/png").await?;
    /// ```
    pub async fn update_avatar_from_bytes(
        &self,
        image_bytes: &[u8],
        content_type: &str,
    ) -> Result<User> {
        let data_uri = Self::image_to_data_uri(image_bytes, content_type);
        self.update_avatar_from_data_uri(data_uri).await
    }

    /// Updates the current user's avatar from a URL (CDN, imgur, etc.)
    ///
    /// # Example
    /// ```ignore
    /// ctx.update_avatar_from_url("https://i.imgur.com/avatar.png").await?;
    /// ctx.update_avatar_from_url("https://cdn.discordapp.com/avatars/123/456.png").await?;
    /// ```
    pub async fn update_avatar_from_url(&self, url: impl AsRef<str>) -> Result<User> {
        let data_uri = self.download_image_as_data_uri(url).await?;
        self.update_avatar_from_data_uri(data_uri).await
    }

    /// Removes the current user's avatar (sets to default)
    pub async fn remove_avatar(&self) -> Result<User> {
        let url = crate::http::api_url("/users/@me");
        let body = json!({
            "avatar": null
        });
        let response = self.http.patch(&url, body).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Universal avatar update - accepts file path OR URL
    ///
    /// Automatically detects if it's a URL or file path
    ///
    /// # Example
    /// ```ignore
    /// ctx.set_avatar("avatar.png").await?;
    /// ctx.set_avatar("https://i.imgur.com/avatar.png").await?;
    /// ```
    pub async fn set_avatar(&self, source: impl AsRef<str>) -> Result<User> {
        let source = source.as_ref();

        if source.starts_with("http://") || source.starts_with("https://") {
            // It's a URL
            self.update_avatar_from_url(source).await
        } else {
            // It's a file path
            self.update_avatar(source).await
        }
    }

    /// Updates the current user's banner from a Data URI
    pub async fn update_banner_from_data_uri(&self, data_uri: impl Into<String>) -> Result<User> {
        let url = crate::http::api_url("/users/@me");
        let body = json!({
            "banner": data_uri.into()
        });
        let response = self.http.patch(&url, body).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Updates the current user's banner from a file path
    ///
    /// # Example
    /// ```ignore
    /// ctx.update_banner("banner.gif").await?;
    /// ```
    pub async fn update_banner(&self, image_path: impl AsRef<Path>) -> Result<User> {
        let data_uri = self.read_image_as_data_uri(image_path).await?;
        self.update_banner_from_data_uri(data_uri).await
    }

    /// Updates the current user's banner from a URL
    ///
    /// # Example
    /// ```ignore
    /// ctx.update_banner_from_url("https://i.imgur.com/banner.gif").await?;
    /// ```
    pub async fn update_banner_from_url(&self, url: impl AsRef<str>) -> Result<User> {
        let data_uri = self.download_image_as_data_uri(url).await?;
        self.update_banner_from_data_uri(data_uri).await
    }

    /// Removes the current user's banner
    pub async fn remove_banner(&self) -> Result<User> {
        let url = crate::http::api_url("/users/@me");
        let body = json!({
            "banner": null
        });
        let response = self.http.patch(&url, body).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Universal banner update - accepts file path OR URL
    pub async fn set_banner(&self, source: impl AsRef<str>) -> Result<User> {
        let source = source.as_ref();

        if source.starts_with("http://") || source.starts_with("https://") {
            // It's a URL
            self.update_banner_from_url(source).await
        } else {
            // It's a file path
            self.update_banner(source).await
        }
    }

    /// Updates multiple user settings at once
    ///
    /// # Example
    /// ```ignore
    /// ctx.update_profile(
    ///     Some("NewUsername"),
    ///     Some("avatar.png"),
    ///     Some("banner.gif"),
    /// ).await?;
    /// ```
    pub async fn update_profile(
        &self,
        username: Option<&str>,
        avatar_path: Option<impl AsRef<Path>>,
        banner_path: Option<impl AsRef<Path>>,
    ) -> Result<User> {
        let mut body = json!({});

        if let Some(name) = username {
            body["username"] = json!(name);
        }

        if let Some(path) = avatar_path {
            let data_uri = self.read_image_as_data_uri(path).await?;
            body["avatar"] = json!(data_uri);
        }

        if let Some(path) = banner_path {
            let data_uri = self.read_image_as_data_uri(path).await?;
            body["banner"] = json!(data_uri);
        }

        let url = crate::http::api_url("/users/@me");
        let response = self.http.patch(&url, body).await?;
        let user: User = serde_json::from_value(response)?;
        Ok(user)
    }

    // ==================== Channel Methods ====================

    /// Gets a channel by ID
    pub async fn get_channel(&self, channel_id: impl AsRef<str>) -> Result<Channel> {
        let url = crate::http::api_url(&format!("/channels/{}", channel_id.as_ref()));
        let response = self.http.get(&url).await?;
        let channel: Channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Sends a message to a channel
    pub async fn send_message(
        &self,
        channel_id: impl AsRef<str>,
        content: impl Into<String>,
    ) -> Result<Message> {
        let url = crate::http::api_url(&format!("/channels/{}/messages", channel_id.as_ref()));
        let body = json!({
            "content": content.into()
        });
        let response = self.http.post(&url, body).await?;
        let message: Message = serde_json::from_value(response)?;
        Ok(message)
    }

    /// Gets a message by channel ID and message ID
    pub async fn get_message(
        &self,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
    ) -> Result<Message> {
        let url = crate::http::api_url(&format!(
            "/channels/{}/messages/{}",
            channel_id.as_ref(),
            message_id.as_ref()
        ));
        let response = self.http.get(&url).await?;
        let message: Message = serde_json::from_value(response)?;
        Ok(message)
    }

    /// Deletes a message
    pub async fn delete_message(
        &self,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
    ) -> Result<()> {
        let url = crate::http::api_url(&format!(
            "/channels/{}/messages/{}",
            channel_id.as_ref(),
            message_id.as_ref()
        ));
        self.http.delete(&url).await?;
        Ok(())
    }

    /// Sends an advanced message (with embeds, replies, tts, etc.)
    pub async fn send_message_advanced(
        &self,
        channel_id: impl AsRef<str>,
        message: CreateMessage,
    ) -> Result<Message> {
        let url = crate::http::api_url(&format!("/channels/{}/messages", channel_id.as_ref()));
        let response = self.http.post(&url, message).await?;
        let msg: Message = serde_json::from_value(response)?;
        Ok(msg)
    }

    /// Gets message history for a channel
    pub async fn get_messages(
        &self,
        channel_id: impl AsRef<str>,
        limit: Option<u32>,
    ) -> Result<Vec<Message>> {
        self.channels
            .get_messages(&self.http, channel_id, limit, None, None, None)
            .await
    }

    /// Purges own messages from a channel.
    /// Fetches messages, filters own, and deletes one by one.
    /// Returns the number of deleted messages.
    pub async fn purge_own_messages(
        &self,
        channel_id: impl AsRef<str>,
        limit: u32,
    ) -> Result<u32> {
        let channel_id = channel_id.as_ref();
        let my_id = &self.user.id;
        let mut deleted = 0u32;
        let mut before: Option<String> = None;

        while deleted < limit {
            let batch_size = std::cmp::min(limit - deleted, 100);
            let messages = self
                .channels
                .get_messages(&self.http, channel_id, Some(batch_size), before.clone(), None, None)
                .await?;

            if messages.is_empty() {
                break;
            }

            before = messages.last().map(|m| m.id.clone());

            for msg in &messages {
                if deleted >= limit {
                    break;
                }
                if msg.author.id == *my_id {
                    self.delete_message(channel_id, &msg.id).await?;
                    deleted += 1;
                }
            }

            if messages.len() < batch_size as usize {
                break;
            }
        }

        Ok(deleted)
    }

    /// Triggers typing indicator in a channel
    pub async fn trigger_typing(&self, channel_id: impl AsRef<str>) -> Result<()> {
        let url = crate::http::api_url(&format!("/channels/{}/typing", channel_id.as_ref()));
        self.http.post(&url, json!({})).await?;
        Ok(())
    }

    // ==================== DM Methods ====================

    /// Creates a DM channel with a user
    pub async fn create_dm(&self, user_id: impl AsRef<str>) -> Result<Channel> {
        let url = crate::http::api_url("/users/@me/channels");
        let body = json!({
            "recipient_id": user_id.as_ref()
        });
        let response = self.http.post(&url, body).await?;
        let channel: Channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Sends a DM to a user
    pub async fn send_dm(
        &self,
        user_id: impl AsRef<str>,
        content: impl Into<String>,
    ) -> Result<Message> {
        let channel = self.create_dm(user_id).await?;
        self.send_message(&channel.id, content).await
    }

    // ==================== Guild Methods ====================

    /// Leaves a guild (server)
    pub async fn leave_guild(&self, guild_id: impl AsRef<str>) -> Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/guilds/{}", guild_id.as_ref()));
        self.http.delete(&url).await?;
        Ok(())
    }

    // ==================== Reaction Methods ====================

    /// Adds a reaction to a message
    pub async fn add_reaction(
        &self,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
        emoji: impl AsRef<str>,
    ) -> Result<()> {
        let url = crate::http::api_url(&format!(
            "/channels/{}/messages/{}/reactions/{}/@me",
            channel_id.as_ref(),
            message_id.as_ref(),
            emoji.as_ref()
        ));
        self.http.put(&url, json!({})).await?;
        Ok(())
    }

    /// Removes a reaction from a message
    pub async fn remove_reaction(
        &self,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
        emoji: impl AsRef<str>,
    ) -> Result<()> {
        let url = crate::http::api_url(&format!(
            "/channels/{}/messages/{}/reactions/{}/@me",
            channel_id.as_ref(),
            message_id.as_ref(),
            emoji.as_ref()
        ));
        self.http.delete(&url).await?;
        Ok(())
    }
}
