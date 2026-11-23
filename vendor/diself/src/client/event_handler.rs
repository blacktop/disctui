use crate::client::{Context, DispatchEvent};
use crate::model::{Message, PassiveUpdateV1, ReadySupplemental, User};
use async_trait::async_trait;
use serde_json::Value;

/// Trait for handling Discord events
///
/// Implement this trait to respond to Discord events.
///
/// # Example
/// ```ignore
/// use diself::prelude::*;
///
/// struct MyBot;
///
/// #[async_trait]
/// impl EventHandler for MyBot {
///     async fn on_ready(&self, ctx: &Context, user: User) {
///         println!("Bot is ready!");
///     }
///     
///     async fn on_message(&self, ctx: &Context, msg: Message) {
///         if msg.content == "!ping" {
///             msg.reply(&ctx.http, "Pong!").await.ok();
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Called for every gateway payload received (all opcodes).
    async fn on_gateway_payload(&self, ctx: &Context, payload: &Value) {
        let _ = (ctx, payload);
    }

    /// Called for every dispatch event (opcode 0), including unknown events.
    async fn on_dispatch(&self, ctx: &Context, event: DispatchEvent) {
        let _ = (ctx, event);
    }

    /// Called when the bot is ready
    async fn on_ready(&self, ctx: &Context, user: User) {
        let _ = (ctx, user);
    }

    /// Called soon after the READY event, and giving extra information about the session
    async fn on_ready_supplemental(&self, ctx: &Context, user: User, data: Value) {
        let _ = (ctx, user, data);
    }

    /// Typed READY_SUPPLEMENTAL callback.
    async fn on_ready_supplemental_typed(
        &self,
        ctx: &Context,
        user: User,
        data: ReadySupplemental,
    ) {
        let _ = (ctx, user, data);
    }

    /// Called for every new message
    async fn on_message_create(&self, ctx: &Context, message: Message) {
        let _ = (ctx, message);
    }

    /// Called when a message is edited
    async fn on_message_update(&self, ctx: &Context, message: Message) {
        let _ = (ctx, message);
    }

    /// Called when a message is deleted
    async fn on_message_delete(&self, ctx: &Context, channel_id: String, message_id: String) {
        let _ = (ctx, channel_id, message_id);
    }

    /// Called when a user is updated
    async fn on_user_update(&self, ctx: &Context, old_user: User, new_user: User) {
        let _ = (ctx, old_user, new_user);
    }

    // ==================== Raw Dispatch Coverage ====================
    // One callback per DispatchEventType (raw JSON payload), discord.js-style coverage.

    async fn on_ready_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_ready_supplemental_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_resumed_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_application_command_permissions_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_auto_moderation_rule_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_auto_moderation_rule_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_auto_moderation_rule_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_auto_moderation_action_execution(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_channel_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_channel_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_channel_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_channel_pins_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_thread_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_thread_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_thread_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_thread_list_sync(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_thread_member_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_thread_members_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_entitlement_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_entitlement_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_entitlement_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_audit_log_entry_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_ban_add(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_ban_remove(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_emojis_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_stickers_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_integrations_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_member_add(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_member_remove(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_member_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_members_chunk(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_role_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_role_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_role_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_scheduled_event_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_scheduled_event_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_scheduled_event_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_scheduled_event_user_add(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_scheduled_event_user_remove(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_soundboard_sound_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_soundboard_sound_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_soundboard_sound_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_guild_soundboard_sounds_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_integration_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_integration_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_integration_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_interaction_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_invite_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_invite_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_create_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_update_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_delete_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_delete_bulk(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_reaction_add(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_reaction_remove(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_reaction_remove_all(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_reaction_remove_emoji(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_poll_vote_add(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_message_poll_vote_remove(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_presence_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_passive_update_v1(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_stage_instance_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_stage_instance_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_stage_instance_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_subscription_create(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_subscription_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_subscription_delete(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_typing_start(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_user_update_event(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_voice_channel_effect_send(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_voice_state_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_voice_server_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_webhooks_update(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_relationship_add(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }
    async fn on_relationship_remove(&self, ctx: &Context, data: Value) {
        let _ = (ctx, data);
    }

    /// Typed PASSIVE_UPDATE_V1 callback.
    async fn on_passive_update_v1_typed(&self, ctx: &Context, data: PassiveUpdateV1) {
        let _ = (ctx, data);
    }
}
