use serde::{Deserialize, Serialize};
use super::Permissions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Role ID
    pub id: String,

    /// Role name
    pub name: String,

    /// DEPRECATED Integer representation of hexadecimal color code (e.g., 0xFF0000 for red)    #[serde(default)]
    pub color: Option<u32>,

    /// The role's color
    #[serde(default)]
    pub colors: Option<RoleColors>,

    /// Icon hash (if the role has an icon)
    pub icon: Option<String>,

    /// Unicode emoji representing the role (if the role has an emoji)
    pub unicode_emoji: Option<String>,

    /// Whether the role is displayed separately in the member list
    #[serde(default)]
    pub hoist: bool,

    /// Position of the role in the role hierarchy (higher numbers are higher roles)
    #[serde(default)]
    pub position: i32,

    /// Permissions bitfield representing the permissions granted by this role
    #[serde(default)]
    pub permissions: Permissions,

    /// Whether the role is managed by an integration (e.g., Twitch, YouTube)
    #[serde(default)]
    pub managed: bool,

    /// Whether the role is mentionable by users without administrator permissions
    #[serde(default)]
    pub mentionable: bool,

    /// Tags for the role (if any)
    #[serde(default)]
    pub tags: Option<RoleTags>,

    /// Role flags (bitfield representing role features)
    pub flags: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleColors {
    /// Primary color of the role (integer representation of hexadecimal color code)
    #[serde(default)]
    pub primary_color: Option<u32>,
    /// Secondary color of the role (integer representation of hexadecimal color code)
    #[serde(default)]
    pub secondary_color: Option<u32>,
    /// Tertiary color of the role (integer representation of hexadecimal color code)
    #[serde(default)]
    pub tertiary_color: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleTags {
    /// The ID of the bot that created this role (if any)
    pub bot_id: Option<String>,

    /// The ID of the integration that created this role (if any)
    pub integration_id: Option<String>,

    /// Whether this is the guild's premium subscriber role
    #[serde(default)]
    pub premium_subscriber: Option<bool>,

    /// The id of this role's subscription SKU and listing
    pub subscription_listing_id: Option<String>,

    /// Whether this role is available for purchase
    #[serde(default)]
    pub available_for_purchase: Option<bool>,

    /// Whether this role is a guild's linked role for an application
    #[serde(default)]
    pub guild_connections: Option<bool>,
}
