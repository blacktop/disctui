use crate::cache::{Cache, CacheConfig};
use crate::client::{ClientBuilder, Context, DispatchEvent, DispatchEventType, EventHandler};
use crate::error::{CaptchaInfo, Result};
use crate::gateway::Gateway;
use crate::http::HttpClient;
use crate::model::{Message, PassiveUpdateV1, ReadySupplemental, User};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Main client struct for the selfbot.   
/// Handles connection to the gateway and dispatching events to the event handler.
/// Also holds an instance of the HTTP client for making API requests.
/// # Example
/// ```ignore
/// use diself::prelude::*;
///
/// struct MyHandler;
/// impl EventHandler for MyHandler {
///     async fn on_ready(&self, ctx: &Context, user: User) {
///         println!("Logged in as {}", user.tag());
///     }
/// }
///
/// let cache_config = CacheConfig {
///     cache_users: true,
///     cache_channels: true,
///     cache_guilds: true,
///     cache_relationships: true,
/// };
///async fn main() {
///     let client = Client::new("your_token_here", MyHandler).with_cache_config(cache_config);
///     // Or
///     // let client = Client::new("your_token_here", MyHandler).without_cache();
///     client.start().await.unwrap();
/// }
///
/// ```
pub struct Client {
    token: String,
    handler: Arc<dyn EventHandler>,
    http: HttpClient,
    cache: Cache,
    client_properties: Option<Value>,
    shutdown_requested: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
}

impl Client {
    pub fn builder<H>(token: impl Into<String>, handler: H) -> Result<ClientBuilder<H>>
    where
        H: EventHandler + 'static,
    {
        ClientBuilder::new(token, handler)
    }

    /// Creates a new client
    pub fn new(token: impl Into<String>, handler: impl EventHandler + 'static) -> Result<Self> {
        let token = token.into();
        let http = HttpClient::new(token.clone())?;
        let cache = Cache::new();
        Ok(Self::from_parts(token, Arc::new(handler), http, cache, None))
    }

    pub(crate) fn from_parts(
        token: String,
        handler: Arc<dyn EventHandler>,
        http: HttpClient,
        cache: Cache,
        client_properties: Option<Value>,
    ) -> Self {
        Self {
            token,
            handler,
            http,
            cache,
            client_properties,
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
        }
    }

    /// Sets cache configuration for this client
    ///
    /// # Example
    /// ```ignore
    /// use diself::prelude::*;
    /// let config = CacheConfig {
    ///     cache_users: true,
    ///     cache_channels: true,
    ///     cache_guilds: true,
    /// };
    /// let client = Client::new(token, MyHandler).with_cache_config(config);
    /// ```
    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.cache = Cache::with_config(config);
        self
    }

    /// Disables caching entirely
    pub fn without_cache(mut self) -> Self {
        self.cache = Cache::with_config(CacheConfig {
            cache_users: false,
            cache_channels: false,
            cache_guilds: false,
            cache_relationships: false,
        });
        self
    }

    /// Sets a captcha handler for this client
    ///
    /// The handler will be called when Discord requires a captcha to be solved.
    /// It should return the solved captcha key.
    ///
    /// # Example   
    /// ```ignore
    /// use diself::prelude::*;
    ///
    /// let client = Client::new(token, MyHandler)
    ///     .with_captcha_handler(|info| async move {
    ///         println!("Captcha required: {:?}", info);
    ///         // Solve captcha and return the key
    ///         Ok("captcha_key_here".to_string())
    ///     });
    /// ```
    pub fn with_captcha_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(CaptchaInfo) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String>> + Send + 'static,
    {
        self.http = self.http.with_captcha_handler(handler);
        self
    }

    /// Returns a reference to the HTTP client
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// Returns a reference to the cache
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Starts the client and listens for events
    pub async fn start(&self) -> Result<()> {
        self.shutdown_requested.store(false, Ordering::SeqCst);
        tracing::info!("Starting Discord client...");

        let mut gateway = Gateway::connect(&self.token, self.client_properties.clone()).await?;

        tracing::info!("Client connected, listening for events...");

        let ctx = Context::create(self.http.clone(), self.cache.clone()).await?;

        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                tracing::info!("Shutdown requested, stopping client loop");
                gateway.shutdown().await?;
                break;
            }

            let next_event = tokio::select! {
                event = gateway.next_event() => Some(event?),
                _ = self.shutdown_notify.notified() => None,
            };

            match next_event {
                Some(event) => {
                    if let Some(event) = event {
                        if event.get("t").and_then(Value::as_str) == Some("READY") {
                            if let Some(guilds) = event["d"]["guilds"].as_array() {
                                gateway.subscribe_to_guilds(guilds).await;
                            }
                        }
                        if let Err(e) = self.handle_event(&ctx, event).await {
                            tracing::error!("Error handling event: {}", e);
                        }
                    } else {
                        tracing::warn!("Gateway connection closed");
                        gateway.shutdown().await?;
                        break;
                    }
                }
                None => {
                    tracing::info!("Shutdown signal received, closing gateway");
                    gateway.shutdown().await?;
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        self.shutdown_notify.notify_waiters();
    }

    async fn handle_event(&self, ctx: &Context, event: Value) -> Result<()> {
        self.handler.on_gateway_payload(ctx, &event).await;

        let op = event.get("op").and_then(|v| v.as_u64());

        // Opcode 0 = Dispatch (events)
        if op == Some(0) {
            if let Some(event_type) = event.get("t").and_then(|v| v.as_str()) {
                let sequence = event.get("s").and_then(|v| v.as_u64());
                let data = event.get("d").cloned().unwrap_or(Value::Null);
                let dispatch = DispatchEvent::from_gateway_payload(event_type, sequence, data);

                let dispatch_kind = dispatch.kind.clone();
                let dispatch_name = dispatch.name().to_string();
                let maybe_old_user = if matches!(dispatch_kind, DispatchEventType::UserUpdate) {
                    dispatch
                        .data
                        .get("id")
                        .and_then(|v| v.as_str())
                        .and_then(|id| ctx.cache.user(id))
                } else {
                    None
                };

                ctx.cache.update_from_dispatch(&dispatch_name, &dispatch.data);
                ctx.collectors.dispatch(dispatch.clone());
                self.handler.on_dispatch(ctx, dispatch.clone()).await;
                self.dispatch_raw_event(ctx, &dispatch).await;

                match dispatch_kind {
                    DispatchEventType::Ready => {
                        if let Some(user) = ctx.cache.current_user() {
                            self.handler.on_ready(ctx, user).await;
                        }
                    }
                    DispatchEventType::ReadySupplemental => {
                        self.handler
                            .on_ready_supplemental(ctx, ctx.user.clone(), dispatch.data.clone())
                            .await;
                        if let Ok(data) =
                            serde_json::from_value::<ReadySupplemental>(dispatch.data.clone())
                        {
                            self.handler
                                .on_ready_supplemental_typed(ctx, ctx.user.clone(), data)
                                .await;
                        }
                    }
                    DispatchEventType::MessageCreate => {
                        if let Ok(message) = serde_json::from_value::<Message>(dispatch.data) {
                            self.handler.on_message_create(ctx, message).await;
                        }
                    }
                    DispatchEventType::MessageUpdate => {
                        if let Ok(message) = serde_json::from_value::<Message>(dispatch.data) {
                            self.handler.on_message_update(ctx, message).await;
                        }
                    }
                    DispatchEventType::MessageDelete => {
                        let data = dispatch.data;
                        if let (Some(channel_id), Some(message_id)) =
                            (data["channel_id"].as_str(), data["id"].as_str())
                        {
                            self.handler
                                .on_message_delete(
                                    ctx,
                                    channel_id.to_string(),
                                    message_id.to_string(),
                                )
                                .await;
                        }
                    }
                    DispatchEventType::UserUpdate => {
                        if let Ok(new_user) = serde_json::from_value::<User>(dispatch.data) {
                            let old_user = maybe_old_user.unwrap_or_else(|| new_user.clone());
                            self.handler.on_user_update(ctx, old_user, new_user).await;
                        }
                    }
                    DispatchEventType::Unknown(name) => {
                        tracing::trace!("Unhandled dispatch event: {}", name);
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    async fn dispatch_raw_event(&self, ctx: &Context, dispatch: &DispatchEvent) {
        match dispatch.kind {
            DispatchEventType::Ready => self.handler.on_ready_event(ctx, dispatch.data.clone()).await,
            DispatchEventType::ReadySupplemental => self
                .handler
                .on_ready_supplemental_event(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::Resumed => self.handler.on_resumed_event(ctx, dispatch.data.clone()).await,
            DispatchEventType::ApplicationCommandPermissionsUpdate => self
                .handler
                .on_application_command_permissions_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::AutoModerationRuleCreate => self
                .handler
                .on_auto_moderation_rule_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::AutoModerationRuleUpdate => self
                .handler
                .on_auto_moderation_rule_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::AutoModerationRuleDelete => self
                .handler
                .on_auto_moderation_rule_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::AutoModerationActionExecution => self
                .handler
                .on_auto_moderation_action_execution(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::ChannelCreate => self.handler.on_channel_create(ctx, dispatch.data.clone()).await,
            DispatchEventType::ChannelUpdate => self.handler.on_channel_update(ctx, dispatch.data.clone()).await,
            DispatchEventType::ChannelDelete => self.handler.on_channel_delete(ctx, dispatch.data.clone()).await,
            DispatchEventType::ChannelPinsUpdate => self
                .handler
                .on_channel_pins_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::ThreadCreate => self.handler.on_thread_create(ctx, dispatch.data.clone()).await,
            DispatchEventType::ThreadUpdate => self.handler.on_thread_update(ctx, dispatch.data.clone()).await,
            DispatchEventType::ThreadDelete => self.handler.on_thread_delete(ctx, dispatch.data.clone()).await,
            DispatchEventType::ThreadListSync => self.handler.on_thread_list_sync(ctx, dispatch.data.clone()).await,
            DispatchEventType::ThreadMemberUpdate => self
                .handler
                .on_thread_member_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::ThreadMembersUpdate => self
                .handler
                .on_thread_members_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::EntitlementCreate => self
                .handler
                .on_entitlement_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::EntitlementUpdate => self
                .handler
                .on_entitlement_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::EntitlementDelete => self
                .handler
                .on_entitlement_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildCreate => self.handler.on_guild_create(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildUpdate => self.handler.on_guild_update(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildDelete => self.handler.on_guild_delete(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildAuditLogEntryCreate => self
                .handler
                .on_guild_audit_log_entry_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildBanAdd => self.handler.on_guild_ban_add(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildBanRemove => self
                .handler
                .on_guild_ban_remove(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildEmojisUpdate => self
                .handler
                .on_guild_emojis_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildStickersUpdate => self
                .handler
                .on_guild_stickers_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildIntegrationsUpdate => self
                .handler
                .on_guild_integrations_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildMemberAdd => self.handler.on_guild_member_add(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildMemberRemove => self
                .handler
                .on_guild_member_remove(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildMemberUpdate => self
                .handler
                .on_guild_member_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildMembersChunk => self
                .handler
                .on_guild_members_chunk(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildRoleCreate => self.handler.on_guild_role_create(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildRoleUpdate => self.handler.on_guild_role_update(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildRoleDelete => self.handler.on_guild_role_delete(ctx, dispatch.data.clone()).await,
            DispatchEventType::GuildScheduledEventCreate => self
                .handler
                .on_guild_scheduled_event_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildScheduledEventUpdate => self
                .handler
                .on_guild_scheduled_event_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildScheduledEventDelete => self
                .handler
                .on_guild_scheduled_event_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildScheduledEventUserAdd => self
                .handler
                .on_guild_scheduled_event_user_add(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildScheduledEventUserRemove => self
                .handler
                .on_guild_scheduled_event_user_remove(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildSoundboardSoundCreate => self
                .handler
                .on_guild_soundboard_sound_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildSoundboardSoundUpdate => self
                .handler
                .on_guild_soundboard_sound_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildSoundboardSoundDelete => self
                .handler
                .on_guild_soundboard_sound_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::GuildSoundboardSoundsUpdate => self
                .handler
                .on_guild_soundboard_sounds_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::IntegrationCreate => self
                .handler
                .on_integration_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::IntegrationUpdate => self
                .handler
                .on_integration_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::IntegrationDelete => self
                .handler
                .on_integration_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::InteractionCreate => self
                .handler
                .on_interaction_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::InviteCreate => self.handler.on_invite_create(ctx, dispatch.data.clone()).await,
            DispatchEventType::InviteDelete => self.handler.on_invite_delete(ctx, dispatch.data.clone()).await,
            DispatchEventType::MessageCreate => self
                .handler
                .on_message_create_event(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageUpdate => self
                .handler
                .on_message_update_event(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageDelete => self
                .handler
                .on_message_delete_event(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageDeleteBulk => self
                .handler
                .on_message_delete_bulk(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageReactionAdd => self
                .handler
                .on_message_reaction_add(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageReactionRemove => self
                .handler
                .on_message_reaction_remove(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageReactionRemoveAll => self
                .handler
                .on_message_reaction_remove_all(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessageReactionRemoveEmoji => self
                .handler
                .on_message_reaction_remove_emoji(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessagePollVoteAdd => self
                .handler
                .on_message_poll_vote_add(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::MessagePollVoteRemove => self
                .handler
                .on_message_poll_vote_remove(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::PresenceUpdate => self
                .handler
                .on_presence_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::PassiveUpdateV1 => self
                .handler
                .on_passive_update_v1(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::StageInstanceCreate => self
                .handler
                .on_stage_instance_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::StageInstanceUpdate => self
                .handler
                .on_stage_instance_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::StageInstanceDelete => self
                .handler
                .on_stage_instance_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::SubscriptionCreate => self
                .handler
                .on_subscription_create(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::SubscriptionUpdate => self
                .handler
                .on_subscription_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::SubscriptionDelete => self
                .handler
                .on_subscription_delete(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::TypingStart => self.handler.on_typing_start(ctx, dispatch.data.clone()).await,
            DispatchEventType::UserUpdate => self.handler.on_user_update_event(ctx, dispatch.data.clone()).await,
            DispatchEventType::VoiceChannelEffectSend => self
                .handler
                .on_voice_channel_effect_send(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::VoiceStateUpdate => self
                .handler
                .on_voice_state_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::VoiceServerUpdate => self
                .handler
                .on_voice_server_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::WebhooksUpdate => self
                .handler
                .on_webhooks_update(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::RelationshipAdd => self
                .handler
                .on_relationship_add(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::RelationshipRemove => self
                .handler
                .on_relationship_remove(ctx, dispatch.data.clone())
                .await,
            DispatchEventType::Unknown(_) => {}
        }

        if let DispatchEventType::PassiveUpdateV1 = dispatch.kind {
            if let Ok(data) = serde_json::from_value::<PassiveUpdateV1>(dispatch.data.clone()) {
                self.handler.on_passive_update_v1_typed(ctx, data).await;
            }
        }
    }
}
