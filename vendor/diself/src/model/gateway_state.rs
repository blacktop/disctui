use serde::{Deserialize, Serialize};

/// READY_SUPPLEMENTAL payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadySupplemental {
    #[serde(default)]
    pub merged_presences: MergedPresences,
    #[serde(default)]
    pub merged_members: Vec<Vec<MergedMember>>,
    #[serde(default)]
    pub guilds: Vec<SupplementalGuildState>,
    #[serde(default)]
    pub user_activities: Vec<serde_json::Value>,
    #[serde(default)]
    pub lazy_private_channels: Vec<serde_json::Value>,
    #[serde(default)]
    pub game_invites: Vec<serde_json::Value>,
    #[serde(default)]
    pub disclose: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergedPresences {
    #[serde(default)]
    pub friends: Vec<serde_json::Value>,
    #[serde(default)]
    pub guilds: Vec<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergedMember {
    pub user_id: String,
    #[serde(default)]
    pub roles: Vec<String>,
    pub premium_since: Option<String>,
    #[serde(default)]
    pub pending: bool,
    pub nick: Option<String>,
    #[serde(default)]
    pub mute: bool,
    pub joined_at: Option<String>,
    #[serde(default)]
    pub flags: u64,
    #[serde(default)]
    pub deaf: bool,
    pub communication_disabled_until: Option<String>,
    pub banner: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplementalGuildState {
    pub id: String,
    #[serde(default)]
    pub voice_states: Vec<serde_json::Value>,
    #[serde(default)]
    pub embedded_activities: Vec<serde_json::Value>,
    #[serde(default)]
    pub activity_instances: Vec<serde_json::Value>,
}

/// PASSIVE_UPDATE_V1 payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveUpdateV1 {
    pub guild_id: String,
    #[serde(default)]
    pub channels: Vec<PassiveChannelState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveChannelState {
    pub id: String,
    pub last_pin_timestamp: Option<String>,
    pub last_message_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadStateContainer {
    #[serde(default)]
    pub entries: Vec<ReadStateEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadStateEntry {
    pub id: String,
    pub read_state_type: Option<u8>,
    pub last_acked_id: Option<String>,
    pub badge_count: Option<u64>,
    pub mention_count: Option<u64>,
    pub last_message_id: Option<String>,
    pub last_viewed: Option<u64>,
    pub last_pin_timestamp: Option<String>,
    pub flags: Option<u64>,
}
