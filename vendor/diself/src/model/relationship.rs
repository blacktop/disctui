use super::user::User;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
/// Represents the type of relationship between the current user and another user (e.g., friend, blocked, etc.)
pub enum RelationshipType {
    None = 0,
    Friend = 1,
    Blocked = 2,
    IncomingRequest = 3,
    OutgoingRequest = 4,
    Implicit = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a relationship between the current user and another user (e.g., friend, blocked, etc.)
pub struct Relationship {
    /// The ID of the related user
    pub id: String,

    /// The type of relationship (e.g., friend, blocked, etc.)
    #[serde(rename = "type")]
    pub kind: RelationshipType,

    /// The user object of the related user (partial user)
    pub user: Option<User>,

    /// The nickname of the related user (if any, only for friends)
    pub nickname: Option<String>,

    /// Whether the friend request was flagged as spam (default false)
    #[serde(default)]
    pub is_spam_request: bool,

    /// Whether the friend request was sent by a user without a mutual friend or small mutual guild (default false)
    #[serde(default)]
    pub stranger_request: bool,

    /// Whether the target user has been ignored by the current user
    #[serde(default)]
    pub user_ignored: bool,

    /// The ID of the application that created the relationship
    pub origin_application_id: Option<String>,

    /// When the user requested a relationship
    pub since: Option<String>,

    /// Whether the target user has authorized the same application the current user's session is associated with
    #[serde(default)]
    pub has_played_game: bool,
}

impl Relationship {
    /// Returns true if this relationship is an accepted friend.
    pub fn is_friend(&self) -> bool {
        self.kind == RelationshipType::Friend
    }

    /// Returns true if the user is blocked.
    pub fn is_blocked(&self) -> bool {
        self.kind == RelationshipType::Blocked
    }

    /// Accepts/sends a friend relationship for this user id.
    pub async fn add_friend(&self, http: &crate::HttpClient) -> crate::Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/relationships/{}", self.id));
        http.put(&url, json!({ "type": 1 })).await?;
        Ok(())
    }

    /// Blocks this user id.
    pub async fn block(&self, http: &crate::HttpClient) -> crate::Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/relationships/{}", self.id));
        http.put(&url, json!({ "type": 2 })).await?;
        Ok(())
    }

    /// Removes this relationship.
    pub async fn remove(&self, http: &crate::HttpClient) -> crate::Result<()> {
        let url = crate::http::api_url(&format!("/users/@me/relationships/{}", self.id));
        http.delete(&url).await?;
        Ok(())
    }
}
