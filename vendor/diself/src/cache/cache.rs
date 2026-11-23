use crate::cache::{CacheConfig, ChannelCache, GuildCache, RelationshipCache, UserCache};
use crate::model::{
    Channel, Guild, MergedMember, Message, PassiveChannelState, PassiveUpdateV1, Presence,
    ReadStateContainer, ReadStateEntry, ReadySupplemental, Relationship, User,
};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;

/// Thread-safe cache for Discord entities
#[derive(Clone)]
pub struct Cache {
    config: CacheConfig,
    user_cache: UserCache,
    channel_cache: ChannelCache,
    guild_cache: GuildCache,
    relationship_cache: RelationshipCache,
    read_states: Arc<DashMap<String, ReadStateEntry>>,
    guild_members: Arc<DashMap<String, Vec<MergedMember>>>,
    passive_channel_states: Arc<DashMap<String, PassiveChannelState>>,
    /// Current user
    current_user: Arc<RwLock<Option<User>>>,
}

impl Cache {
    /// Creates a new cache with default configuration
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Creates a new cache with custom configuration
    pub fn with_config(config: CacheConfig) -> Self {
        Self {
            user_cache: UserCache::new(config.cache_users),
            channel_cache: ChannelCache::new(config.cache_channels),
            guild_cache: GuildCache::new(config.cache_guilds),
            relationship_cache: RelationshipCache::new(config.cache_relationships),
            read_states: Arc::new(DashMap::new()),
            guild_members: Arc::new(DashMap::new()),
            passive_channel_states: Arc::new(DashMap::new()),
            config,
            current_user: Arc::new(RwLock::new(None)),
        }
    }
    // ==================== Initialization ====================

    /// Initializes the caches with data from the READY event
    pub fn initialize(&self, data: serde_json::Value) {
        if let Ok(user) = serde_json::from_value::<User>(data["user"].clone()) {
            if user.bot {
                eprintln!("discord-selfbot-rs is intended for use with user accounts. If you are seeing this message, it means you have logged in with a bot token, which is not supported. Please log in with a user token instead.");
            }

            self.set_current_user(user);
        }
        self.initialize_users(data["users"].clone());
        self.initialize_guilds(data["guilds"].clone());
        self.initialize_relationships(data["relationships"].clone());
        self.initialize_read_states(data["read_state"].clone());
    }

    /// Updates cache state from one gateway dispatch event payload.
    ///
    /// `event_type` must be the raw gateway dispatch event name, for example `MESSAGE_CREATE`.
    pub fn update_from_dispatch(&self, event_type: &str, data: &Value) {
        match event_type {
            "READY" => self.initialize(data.clone()),
            "READY_SUPPLEMENTAL" => self.update_presence_from_ready_supplemental(data),
            "PASSIVE_UPDATE_V1" => self.update_from_passive_update(data),
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
                if let Ok(channel) = serde_json::from_value::<Channel>(data.clone()) {
                    self.cache_channel(channel);
                }
            }
            "CHANNEL_DELETE" | "THREAD_DELETE" => {
                if let Some(channel_id) = data.get("id").and_then(|v| v.as_str()) {
                    self.remove_channel(channel_id);
                }
            }
            "THREAD_LIST_SYNC" => {
                if let Some(threads) = data.get("threads").and_then(|v| v.as_array()) {
                    for thread in threads {
                        if let Ok(channel) = serde_json::from_value::<Channel>(thread.clone()) {
                            self.cache_channel(channel);
                        }
                    }
                }
            }
            "GUILD_CREATE" | "GUILD_UPDATE" => {
                if let Ok(guild) = serde_json::from_value::<Guild>(data.clone()) {
                    for channel in &guild.channels {
                        self.cache_channel(channel.clone());
                    }
                    for member in &guild.members {
                        self.cache_user(member.user.clone());
                    }
                    self.cache_guild(guild);
                }
            }
            "GUILD_DELETE" => {
                if let Some(guild_id) = data.get("id").and_then(|v| v.as_str()) {
                    self.remove_guild(guild_id);
                }
            }
            "RELATIONSHIP_ADD" => {
                if let Ok(relationship) = serde_json::from_value::<Relationship>(data.clone()) {
                    self.cache_relationship(relationship);
                }
            }
            "RELATIONSHIP_REMOVE" => {
                if let Some(user_id) = data.get("id").and_then(|v| v.as_str()) {
                    self.remove_relationship(user_id);
                }
            }
            "MESSAGE_CREATE" | "MESSAGE_UPDATE" => {
                if let Ok(message) = serde_json::from_value::<Message>(data.clone()) {
                    self.cache_user(message.author);
                    for user in message.mentions {
                        self.cache_user(user);
                    }
                    if let Some(thread) = message.thread {
                        self.cache_channel(thread);
                    }
                }
            }
            "USER_UPDATE" => {
                self.upsert_user_from_partial(data);
                if let Some(user_id) = data.get("id").and_then(|v| v.as_str()) {
                    if let Some(current) = self.current_user() {
                        if current.id == user_id {
                            if let Some(updated) = self.user(user_id) {
                                self.set_current_user(updated);
                            }
                        }
                    }
                }
            }
            "PRESENCE_UPDATE" => {
                if let Some(user_payload) = data.get("user") {
                    self.upsert_user_from_partial(user_payload);
                }
                self.update_user_presence_from_event(data);
            }
            "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => {
                if let Some(user_payload) = data.get("user") {
                    self.upsert_user_from_partial(user_payload);
                }
            }
            "GUILD_MEMBERS_CHUNK" => {
                if let Some(members) = data.get("members").and_then(|v| v.as_array()) {
                    for member in members {
                        if let Some(user_payload) = member.get("user") {
                            self.upsert_user_from_partial(user_payload);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ==================== Current User ====================

    /// Gets the current user from cache
    pub fn current_user(&self) -> Option<User> {
        self.current_user.read().clone()
    }

    /// Sets the current user in cache
    pub fn set_current_user(&self, user: User) {
        *self.current_user.write() = Some(user.clone());
        self.user_cache.insert(user);
    }

    // ==================== Users ====================

    /// Initializes user cache with data from the READY event
    pub fn initialize_users(&self, data: serde_json::Value) {
        self.user_cache.initialize_from_ready(data);
    }

    /// Gets a user from cache by ID
    pub fn user(&self, user_id: &str) -> Option<User> {
        self.user_cache.get(user_id)
    }

    /// Inserts or updates a user in cache
    pub fn cache_user(&self, user: User) {
        self.user_cache.insert(user);
    }

    /// Removes a user from cache
    pub fn remove_user(&self, user_id: &str) -> Option<User> {
        self.user_cache.remove(user_id)
    }

    /// Returns the number of cached users
    pub fn user_count(&self) -> usize {
        self.user_cache.count()
    }

    /// Gets all cached users
    pub fn users(&self) -> Vec<User> {
        self.user_cache.all()
    }

    // ==================== Relationships ====================

    /// Initalizes relationships cache with data from the READY event
    pub fn initialize_relationships(&self, data: serde_json::Value) {
        self.relationship_cache.initialize_from_ready(data);
    }

    /// Gets a relationship from cache by user ID
    pub fn relationship(&self, user_id: &str) -> Option<Relationship> {
        self.relationship_cache.get(user_id)
    }

    /// Inserts or updates a relationship in cache
    pub fn cache_relationship(&self, relationship: Relationship) {
        self.relationship_cache.insert(relationship);
    }

    /// Removes a relationship from cache
    pub fn remove_relationship(&self, user_id: &str) -> Option<Relationship> {
        self.relationship_cache.remove(user_id)
    }

    /// Returns the number of cached relationships
    pub fn relationship_count(&self) -> usize {
        self.relationship_cache.count()
    }

    /// Gets friends from cache
    pub fn friends(&self) -> Vec<Relationship> {
        self.relationship_cache.friends()
    }

    // ==================== Read States ====================

    /// Initializes read-state cache from the READY event's `read_state` payload.
    pub fn initialize_read_states(&self, data: Value) {
        let container = serde_json::from_value::<ReadStateContainer>(data).unwrap_or_default();
        self.read_states.clear();
        for entry in container.entries {
            self.read_states.insert(entry.id.clone(), entry);
        }
    }

    /// Gets one read-state entry by channel or guild id.
    pub fn read_state(&self, id: &str) -> Option<ReadStateEntry> {
        self.read_states.get(id).map(|entry| entry.value().clone())
    }

    /// Gets all read-state entries.
    pub fn read_states(&self) -> Vec<ReadStateEntry> {
        self.read_states
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    // ==================== Channels ====================

    /// Gets a channel from cache by ID
    pub fn channel(&self, channel_id: &str) -> Option<Channel> {
        self.channel_cache.get(channel_id)
    }

    /// Inserts or updates a channel in cache
    pub fn cache_channel(&self, channel: Channel) {
        self.channel_cache.insert(channel);
    }

    /// Removes a channel from cache
    pub fn remove_channel(&self, channel_id: &str) -> Option<Channel> {
        self.channel_cache.remove(channel_id)
    }

    /// Returns the number of cached channels
    pub fn channel_count(&self) -> usize {
        self.channel_cache.count()
    }

    /// Gets all cached channels
    pub fn channels(&self) -> Vec<Channel> {
        self.channel_cache.all()
    }

    // ==================== Guilds ====================

    /// Initializes guild cache with data from the READY event
    pub fn initialize_guilds(&self, data: serde_json::Value) {
        self.channel_cache.initialize_from_ready(data.clone());
        self.guild_cache.initialize_from_ready(data);
    }

    /// Gets a guild from cache by ID
    pub fn guild(&self, guild_id: &str) -> Option<Guild> {
        self.guild_cache.get(guild_id)
    }

    /// Inserts or updates a guild in cache
    pub fn cache_guild(&self, guild: Guild) {
        self.guild_cache.insert(guild);
    }

    /// Removes a guild from cache
    pub fn remove_guild(&self, guild_id: &str) -> Option<Guild> {
        self.guild_cache.remove(guild_id)
    }

    /// Returns the number of cached guilds
    pub fn guild_count(&self) -> usize {
        self.guild_cache.count()
    }

    /// Gets all cached guilds
    pub fn guilds(&self) -> Vec<Guild> {
        self.guild_cache.all()
    }

    // ==================== Supplemental Guild Members ====================

    /// Gets merged supplemental members by guild id.
    pub fn guild_members(&self, guild_id: &str) -> Vec<MergedMember> {
        self.guild_members
            .get(guild_id)
            .map(|entry| entry.value().clone())
            .unwrap_or_default()
    }

    /// Gets one merged supplemental member by guild id and user id.
    pub fn guild_member(&self, guild_id: &str, user_id: &str) -> Option<MergedMember> {
        self.guild_members(guild_id)
            .into_iter()
            .find(|member| member.user_id == user_id)
    }

    // ==================== Passive Channel States ====================

    /// Gets passive channel state by channel id.
    pub fn passive_channel_state(&self, channel_id: &str) -> Option<PassiveChannelState> {
        self.passive_channel_states
            .get(channel_id)
            .map(|entry| entry.value().clone())
    }

    // ==================== Cache Management ====================

    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// Clears all caches
    pub fn clear(&self) {
        self.user_cache.clear();
        self.channel_cache.clear();
        self.guild_cache.clear();
        self.relationship_cache.clear();
        self.read_states.clear();
        self.guild_members.clear();
        self.passive_channel_states.clear();
        *self.current_user.write() = None;
    }

    /// Clears only the user cache
    pub fn clear_users(&self) {
        self.user_cache.clear();
    }

    /// Clears only the channel cache
    pub fn clear_channels(&self) {
        self.channel_cache.clear();
    }

    /// Clears only the guild cache
    pub fn clear_guilds(&self) {
        self.guild_cache.clear();
    }

    /// Clears only the relationship cache
    pub fn clear_relationships(&self) {
        self.relationship_cache.clear();
    }

    /// Gets cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            users: self.user_count(),
            channels: self.channel_count(),
            guilds: self.guild_count(),
        }
    }

    fn upsert_user_from_partial(&self, partial: &Value) {
        if let Ok(user) = serde_json::from_value::<User>(partial.clone()) {
            self.cache_user(user);
            return;
        }

        let Some(user_id) = partial.get("id").and_then(|v| v.as_str()) else {
            return;
        };
        let Some(existing) = self.user(user_id) else {
            return;
        };

        let mut merged = match serde_json::to_value(existing) {
            Ok(value) => value,
            Err(_) => return,
        };

        merge_object_values(&mut merged, partial);

        if let Ok(user) = serde_json::from_value::<User>(merged) {
            self.cache_user(user);
        }
    }

    fn update_user_presence_from_event(&self, presence_event: &Value) {
        let Some(user_id) = presence_event
            .get("user")
            .and_then(|user| user.get("id"))
            .and_then(|v| v.as_str())
        else {
            return;
        };

        let Some(mut user) = self.user(user_id) else {
            return;
        };

        if let Some(presence) = parse_presence_event(presence_event) {
            user.presence = Some(presence);
            self.cache_user(user.clone());

            if self
                .current_user()
                .as_ref()
                .map(|current| current.id == user_id)
                .unwrap_or(false)
            {
                self.set_current_user(user);
            }
        }
    }

    fn update_presence_from_ready_supplemental(&self, data: &Value) {
        let Ok(ready_supplemental) = serde_json::from_value::<ReadySupplemental>(data.clone())
        else {
            return;
        };

        for entry in &ready_supplemental.merged_presences.friends {
            self.update_user_presence_from_merged_entry(entry);
        }

        for entries in &ready_supplemental.merged_presences.guilds {
            for entry in entries {
                self.update_user_presence_from_merged_entry(entry);
            }
        }

        if let Some(guilds) = data.get("guilds").and_then(|v| v.as_array()) {
            for (idx, guild_payload) in guilds.iter().enumerate() {
                let Some(guild_id) = guild_payload.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let members = ready_supplemental
                    .merged_members
                    .get(idx)
                    .cloned()
                    .unwrap_or_default();
                if !members.is_empty() {
                    self.guild_members.insert(guild_id.to_string(), members);
                }
            }
        }
    }

    fn update_from_passive_update(&self, data: &Value) {
        let Ok(payload) = serde_json::from_value::<PassiveUpdateV1>(data.clone()) else {
            return;
        };

        for state in payload.channels {
            if let Some(mut channel) = self.channel(&state.id) {
                channel.last_message_id = state.last_message_id.clone();
                channel.last_pin_timestamp = state.last_pin_timestamp.clone();
                self.cache_channel(channel);
            }
            self.passive_channel_states.insert(state.id.clone(), state);
        }
    }

    fn update_user_presence_from_merged_entry(&self, entry: &Value) {
        let Some(user_id) = entry.get("user_id").and_then(|v| v.as_str()) else {
            return;
        };

        let Some(mut user) = self.user(user_id) else {
            return;
        };

        if let Some(presence) = parse_merged_presence_entry(entry) {
            user.presence = Some(presence);
            self.cache_user(user.clone());

            if self
                .current_user()
                .as_ref()
                .map(|current| current.id == user_id)
                .unwrap_or(false)
            {
                self.set_current_user(user);
            }
        }
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the cache
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub users: usize,
    pub channels: usize,
    pub guilds: usize,
}

fn merge_object_values(target: &mut Value, patch: &Value) {
    let Some(target_obj) = target.as_object_mut() else {
        return;
    };
    let Some(patch_obj) = patch.as_object() else {
        return;
    };

    for (key, value) in patch_obj {
        target_obj.insert(key.clone(), value.clone());
    }
}

fn parse_presence_event(event: &Value) -> Option<Presence> {
    let status = event.get("status").and_then(|v| v.as_str())?.to_string();
    let activities = event
        .get("activities")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let client_status = event.get("client_status").cloned();
    let since = event.get("since").and_then(|v| v.as_i64());
    let afk = event.get("afk").and_then(|v| v.as_bool());

    Some(Presence {
        status,
        activities,
        client_status: client_status.and_then(|v| serde_json::from_value(v).ok()),
        since,
        afk,
    })
}

fn parse_merged_presence_entry(entry: &Value) -> Option<Presence> {
    let status = entry.get("status").and_then(|v| v.as_str())?.to_string();
    let activities = entry
        .get("activities")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let client_status = entry.get("client_status").cloned();
    let afk = entry.get("afk").and_then(|v| v.as_bool());

    Some(Presence {
        status,
        activities,
        client_status: client_status.and_then(|v| serde_json::from_value(v).ok()),
        since: None,
        afk,
    })
}
