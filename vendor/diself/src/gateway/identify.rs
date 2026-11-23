use serde::{Deserialize, Serialize};
use std::fmt;

//Authentication payload for Discord Gateway
#[derive(Clone, Serialize, Deserialize)]
pub struct Identify {
    /// User Discord token
    pub token: String,

    /// Client properties: the session's own, or the application identity.
    pub properties: serde_json::Value,

    /// Initial presence (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<PresenceUpdate>,

    /// Compression (must be false for selfbots)
    pub compress: Option<bool>,

    /// Client capability (essential for selfbots)
    pub capabilities: u32,

    /// Gateway intents (what events we want to receive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intents: Option<u32>,
}

impl fmt::Debug for Identify {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Identify")
            .field("token", &"[REDACTED]")
            .field("properties", &self.properties)
            .field("presence", &self.presence)
            .field("compress", &self.compress)
            .field("capabilities", &self.capabilities)
            .field("intents", &self.intents)
            .finish()
    }
}

// Connection properties sent in the Identify payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProperties {
    /// Operation System (e.g., "Windows", "Linux", "macOS")
    pub os: String,

    /// Library or application name.
    pub browser: String,

    /// Device (empty for desktops)
    pub device: String,

    /// Sytem locale (e.g., "en-US")
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system_locale: String,

    /// Navigator version (e.g., "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
    #[serde(skip_serializing_if = "String::is_empty")]
    pub browser_version: String,

    /// OS version (e.g., "10")
    #[serde(skip_serializing_if = "String::is_empty")]
    pub os_version: String,

    /// Referrer (empty for selfbots)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub referrer: String,

    /// Referring domain (empty for selfbots)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub referring_domain: String,

    /// Release channel (e.g., "stable")
    #[serde(skip_serializing_if = "String::is_empty")]
    pub release_channel: String,

    /// Client build number (e.g., 9999)
    #[serde(skip_serializing_if = "is_zero")]
    pub client_build_number: u32,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl ConnectionProperties {
    // Returning properties with default values (can be customized if needed)
    pub fn default_client() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            browser: "disctui".to_string(),
            device: "disctui".to_string(),
            system_locale: String::new(),
            browser_version: String::new(),
            os_version: String::new(),
            referrer: "".to_string(),
            referring_domain: "".to_string(),
            release_channel: String::new(),
            client_build_number: 0,
        }
    }
}

/// Presence update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUpdate {
    /// Status (e.g., "online", "idle", "dnd", "invisible")
    pub status: String,

    /// Timestamps
    pub since: Option<u64>,

    /// Activities
    pub activities: Vec<Activity>,

    /// AFK status
    pub afk: bool,
}

impl Default for PresenceUpdate {
    fn default() -> Self {
        Self {
            status: "online".to_string(),
            since: None,
            activities: vec![],
            afk: false,
        }
    }
}

// Activtiy (playing, streaming, listening, watching)
#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct Activity {
    // Name of the activity (e.g., "Spotify")
    pub name: String,

    // Type of activity (0 = playing, 1 = streaming, 2 = listening, 3 = watching)
    #[serde(rename = "type")]
    pub kind: u8,

    // URL for streaming (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Identify {
    /// Builds an Identify for `token`.
    ///
    /// `client_properties` are the properties of the client that created the
    /// session. Without them the application identity is sent.
    pub fn new(
        token: impl Into<String>,
        client_properties: Option<&serde_json::Value>,
    ) -> serde_json::Result<Self> {
        let properties = match client_properties {
            Some(properties) => properties.clone(),
            None => serde_json::to_value(ConnectionProperties::default_client())?,
        };
        Ok(Self {
            token: token.into(),
            properties,
            presence: None,
            compress: Some(false),
            capabilities: 16381, // Standard capabilities for Discord clients
            intents: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::gateway::identify::Identify;

    #[test]
    fn debug_output_redacts_the_session_token() {
        let identify = Identify::new("sensitive-test-value", None).expect("identify");
        let text = format!("{identify:?}");
        assert!(!text.contains("sensitive-test-value"));
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn identify_uses_current_property_names_and_the_application_identity() {
        let identify = Identify::new("fixture", None).expect("identify");
        let payload = serde_json::to_value(identify).expect("serialize");
        let properties = payload
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("properties");
        assert_eq!(properties.len(), 3);
        assert_eq!(
            properties.get("os").and_then(serde_json::Value::as_str),
            Some(std::env::consts::OS)
        );
        assert_eq!(
            properties
                .get("browser")
                .and_then(serde_json::Value::as_str),
            Some("disctui")
        );
        assert_eq!(
            properties.get("device").and_then(serde_json::Value::as_str),
            Some("disctui")
        );
        assert!(payload.get("intents").is_none());
        assert!(payload.get("presence").is_none());
    }

    #[test]
    fn identify_sends_the_session_client_properties_unchanged() {
        let session = serde_json::json!({
            "os": "Mac OS X",
            "browser": "Safari",
            "client_build_number": 1,
            "client_event_source": null,
        });
        let identify = Identify::new("fixture", Some(&session)).expect("identify");
        let payload = serde_json::to_value(identify).expect("serialize");
        assert_eq!(payload.get("properties"), Some(&session));
    }
}
