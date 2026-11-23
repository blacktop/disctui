use super::embed::Embed;
use super::embed_builder::EmbedBuilder;
use serde::Serialize;

/// Builder for creating Discord messages with embeds, replies, etc.
///
/// # Example
/// ```
/// use diself::model::{CreateMessage, EmbedBuilder};
///
/// let msg = CreateMessage::new()
///     .content("Hello!")
///     .embed(EmbedBuilder::new().title("Embed").build())
///     .tts(true);
/// ```
#[derive(Debug, Clone, Default, Serialize)]
pub struct CreateMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tts: Option<bool>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<Embed>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_reference: Option<MessageReferencePayload>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageReferencePayload {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(default)]
    pub fail_if_not_exists: bool,
}

impl CreateMessage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn tts(mut self, tts: bool) -> Self {
        self.tts = Some(tts);
        self
    }

    pub fn embed(mut self, embed: Embed) -> Self {
        self.embeds.push(embed);
        self
    }

    pub fn embed_builder(self, builder: EmbedBuilder) -> Self {
        self.embed(builder.build())
    }

    pub fn reply_to(mut self, message_id: impl Into<String>) -> Self {
        self.message_reference = Some(MessageReferencePayload {
            message_id: message_id.into(),
            channel_id: None,
            guild_id: None,
            fail_if_not_exists: false,
        });
        self
    }

    pub fn nonce(mut self, nonce: impl Into<String>) -> Self {
        self.nonce = Some(nonce.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_message_basic() {
        let msg = CreateMessage::new().content("Hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["content"], "Hello");
        assert!(json.get("tts").is_none());
        assert!(json.get("message_reference").is_none());
    }

    #[test]
    fn test_create_message_with_embed() {
        let msg = CreateMessage::new()
            .content("Check this out")
            .embed_builder(EmbedBuilder::new().title("Title").color(0x00FF00));
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["embeds"][0]["title"], "Title");
        assert_eq!(json["embeds"][0]["color"], 0x00FF00);
    }

    #[test]
    fn test_create_message_reply() {
        let msg = CreateMessage::new()
            .content("Reply!")
            .reply_to("123456789");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["message_reference"]["message_id"], "123456789");
    }

    #[test]
    fn test_create_message_tts() {
        let msg = CreateMessage::new().content("Loud").tts(true);
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["tts"], true);
    }
}
