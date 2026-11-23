use crate::model::{Emoji, Member};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// User unique ID
    pub id: String,

    /// Username
    pub username: String,

    /// Discriminator (deprecated)
    pub discriminator: String,

    /// User's display name (if any)
    pub global_name: Option<String>,

    // Avatar hash
    pub avatar: Option<String>,

    #[serde(default)]
    /// Whether the user is a bot
    pub bot: bool,

    #[serde(default)]
    /// Whether the user is a system user (e.g., official Discord accounts)
    pub system: bool,

    // Whether the user has MFA enabled
    #[serde(default)]
    pub mfa_enabled: bool,

    /// User's banner hash (if any)
    pub banner: Option<String>,

    /// User's accent color (if any)
    pub accent_color: Option<u32>,

    /// User's locale (e.g., "en-US")
    pub locale: Option<String>,

    /// Whether the user has verified their email (only for the current user)
    pub verified: Option<bool>,

    /// Email (only for the current user, requires "email" scope)
    pub email: Option<String>,

    /// Phone number (only for the current user, requires "phone" scope)
    pub phone: Option<String>,

    /// Whether the use has used the desktop client before
    #[serde(default)]
    pub desktop: bool,

    /// Whether the user has used the mobile client before
    #[serde(default)]
    pub mobile: bool,

    /// Flags (bitfield representing user features)
    pub flags: Option<u64>,

    /// Premium type (0 = none, 1 = Nitro Classic, 2 = Nitro)
    pub premium_type: Option<u8>,

    /// Public flags (bitfield representing public user features)
    pub public_flags: Option<u64>,

    /// Avatar decoration data (if any)
    pub avatar_decoration: Option<AvatarDecoration>,

    /// Data for the user's collectibles (if any)
    #[serde(default)]
    pub collectibles: Option<Nameplate>,

    /// The user's primary guild
    #[serde(default)]
    pub primary_guild: Option<PrimaryGuild>,

    /// Live presence data when available from gateway events.
    #[serde(default)]
    pub presence: Option<Presence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    /// Online status (`online`, `idle`, `dnd`, `offline`, ...)
    pub status: String,

    /// Current activities as raw payload entries.
    #[serde(default)]
    pub activities: Vec<serde_json::Value>,

    /// Client platform statuses (`desktop`, `mobile`, `web`) when provided.
    pub client_status: Option<ClientStatus>,

    /// Unix time (ms) when user went idle, if any.
    pub since: Option<i64>,

    /// AFK flag from gateway presence payload.
    pub afk: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientStatus {
    pub desktop: Option<String>,
    pub mobile: Option<String>,
    pub web: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// The bot's application profile
    pub application: Option<ApplicationProfile>,

    /// The user's profile user data
    pub user: Option<User>,

    /// The user's profile metadata
    #[serde(default)]
    pub user_profile: Option<ProfileMetadata>,

    /// The user's profile badges
    #[serde(default)]
    pub badges: Option<Vec<Badge>>,

    /// The guild member in the guild specified
    #[serde(default)]
    pub guild_member: Option<Member>,

    /// The guild member's profile in the guild specified
    #[serde(default)]
    pub guild_member_profile: Option<ProfileMetadata>,

    /// The user's pre-migration username#discriminator, if applicable and shown
    pub legacy_username: Option<String>,

    /// The mutual guilds of the user with the current user
    #[serde(default)]
    pub mutual_guilds: Option<Vec<MutualGuild>>,

    /// The mutual friends the user has with the current user
    #[serde(default)]
    pub mutual_friends: Option<Vec<User>>,

    /// The number of mutual friends the user has with the current user
    pub mutual_friend_count: Option<u64>,

    /// The type of premium (Nitro) subscription on a user's account
    pub premium_type: Option<u8>,

    /// The date the user's premium subscription started
    pub premium_since: Option<String>,

    /// The date the user's premium guild (boosting) subscription started
    pub premium_guild_since: Option<String>,
}

impl User {
    /// Returns the user's tag (username#discriminator)
    pub fn tag(&self) -> String {
        format!("{}#{}", self.username, self.discriminator)
    }

    /// Returns the URL of the user's avatar (if any)
    pub fn avatar_url(&self) -> Option<String> {
        self.avatar.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/avatars/{}/{}.png",
                self.id, hash
            )
        })
    }
    /// Returns the URL of the user's banner (if any)
    pub fn banner_url(&self) -> Option<String> {
        self.banner.as_ref().map(|hash| {
            let extension = if hash.starts_with("a_") { "gif" } else { "png" };
            format!(
                "https://cdn.discordapp.com/banners/{}/{}.{}",
                self.id, hash, extension
            )
        })
    }

    /// Returns a string representation of the user's mention (e.g., "<@123456789>")
    pub fn mention(&self) -> String {
        format!("<@{}>", self.id)
    }

    /// Checks if the user has any form of Nitro subscription
    pub fn has_nitro(&self) -> bool {
        matches!(self.premium_type, Some(1) | Some(2) | Some(3))
    }

    /// Returns a human-readable name for the user's Nitro subscription (if any)
    pub fn premium_type_name(&self) -> &str {
        match self.premium_type {
            Some(1) => "Nitro Classic",
            Some(2) => "Nitro",
            Some(3) => "Nitro Basic",
            _ => "None",
        }
    }

    /// Sends a friend request to this user.
    pub async fn add_friend(&self, http: &crate::HttpClient) -> crate::Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/relationships/{}", self.id));
        http.put(&url, json!({ "type": 1 })).await?;
        Ok(())
    }

    /// Blocks this user.
    pub async fn block(&self, http: &crate::HttpClient) -> crate::Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/relationships/{}", self.id));
        http.put(&url, json!({ "type": 2 })).await?;
        Ok(())
    }

    /// Removes any relationship with this user (friend, blocked, pending...).
    pub async fn remove_relationship(&self, http: &crate::HttpClient) -> crate::Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/relationships/{}", self.id));
        http.delete(&url).await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Badge {
    /// The reference ID of the badge
    pub id: String,

    /// A description of the badge
    pub description: String,

    /// The badge's icon hash
    pub icon: Option<String>,

    /// A link representing the badge
    pub link: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMetadata {
    /// The guild ID this profile applies to, if it is a guild profile
    pub guild_id: Option<String>,

    /// The user's pronouns (max 40 characters)
    pub pronouns: Option<String>,

    /// The user's bio
    pub bio: Option<String>,

    /// The user's banner hash
    pub banner: Option<String>,

    /// The user's banner accent color
    pub accent_color: Option<u32>,

    /// The user's profile theme (currently unused)
    pub theme_colors: Option<Vec<u32>>,

    /// The user's profile popout animation particle type
    pub popout_animation_particle_type: Option<u8>,

    /// The user's profile emoji
    #[serde(default)]
    pub emoji: Option<Emoji>,

    ///  The user's profile effect
    #[serde(default)]
    pub profile_effect: Option<ProfileEffect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutualGuild {
    /// The guild ID
    pub id: String,
    /// The user's nickname in the guild
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileEffect {
    /// The profile effect's ID
    pub id: String,

    /// Unix timestamp of when the current profile effect expires
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationProfile {
    /// The application's ID
    pub id: String,

    /// The application's flags
    pub flags: Option<u64>,

    /// Whether the application is verified
    #[serde(default)]
    pub verified: bool,

    /// Whether the application has monetization enabled
    #[serde(default)]
    pub storefront_available: bool,

    /// The ID of the application's primary SKU (if any)
    pub primary_sku_id: Option<String>,

    /// The default in-app authorization link for the intergation
    pub install_params: Option<String>,

    /// The ID of the application's most popular application commands (max 5)
    pub popular_application_command_ids: Option<Vec<String>>,

    /// The application's default custom authorization link (if any)
    pub custom_install_url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarDecoration {
    /// The avatar decoration hash
    pub asset: Option<String>,

    /// ID of the avatar decoration's SKU (if any)
    pub sku_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nameplate {
    /// ID of the nameplate SKU
    pub sku_id: Option<String>,

    /// Path to the nameplate asset
    pub asset: Option<String>,

    /// The label of this nameplate (Currently unused)
    pub label: Option<String>,

    /// Background color of the nameplate (crimson, berry, sky, teal, forest, bubble_gum, violet, cobalt, clover, lemon, white)
    pub palette: Option<String>,

    /// Unix timestamp of when the current nameplate expires (if any)
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimaryGuild {
    /// User's primary guild ID
    pub identity_guild_id: Option<String>,

    /// Whether the user is displaying the primary guild's server tag. This can be null if the system clears the identity, e.g. the server no longer supports tags. This will be false if the user manually removes their tag.
    pub identity_enabled: Option<bool>,

    /// The text of the user's server tag. Limited to 4 characters.
    pub tag: Option<String>,

    /// The sevrer tag badge hash
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Avatar {
    /// The avatar ID
    pub id: Option<String>,

    /// The avatar hash
    pub storage_hash: Option<String>,

    /// The avatar's description (if any)
    pub description: Option<String>,
}
