use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;

use crate::model::{ChannelDigest, MessageRow, TodoItem};

pub fn build_system_prompt(channel_name: &str, user_name: &str) -> String {
    format!(
        "You are a Discord conversation summarizer for channel #{channel_name}. \
         The user's name is \"{user_name}\". \
         Summarize the conversation concisely. \
         Identify messages that may need {user_name}'s attention or reply. \
         Focus on: direct mentions/questions to {user_name}, decisions that need input, \
         action items, and important announcements. \
         Be concise and factual. \
         Respond ONLY with a JSON object (no markdown, no explanation) matching this schema:\n\
         {RESPONSE_SCHEMA}"
    )
}

/// JSON Schema for the digest response, shared by every backend that supports
/// schema-constrained output.
pub fn digest_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "headline": { "type": "string" },
            "summary": { "type": "string" },
            "action_items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "author": { "type": "string" },
                        "snippet": { "type": "string" },
                        "reason": { "type": "string" },
                        "message_id": { "type": "string" }
                    },
                    "required": ["author", "snippet", "reason", "message_id"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["headline", "summary", "action_items"],
        "additionalProperties": false
    })
}

pub fn format_messages(messages: &[MessageRow]) -> String {
    let mut lines = Vec::with_capacity(messages.len());
    for msg in messages {
        let prefix = if msg.is_continuation {
            "  "
        } else {
            &msg.author
        };
        lines.push(format!(
            "[{id}] [{timestamp}] {prefix}: {content}",
            id = msg.id,
            timestamp = msg.timestamp,
            content = msg.content,
        ));
    }
    lines.join("\n")
}

pub fn parse_digest(text: &str, channel_id: &str) -> Result<ChannelDigest> {
    let json_str = extract_json(text);

    let raw: RawDigest = serde_json::from_str(json_str)
        .map_err(|e| eyre!("failed to parse digest JSON: {e}\nraw: {text}"))?;

    let headline = match raw.headline.trim() {
        "" => headline_from_summary(&raw.summary),
        headline => headline.to_string(),
    };

    Ok(ChannelDigest {
        channel_id: channel_id.into(),
        headline,
        summary: raw.summary,
        todos: raw
            .action_items
            .into_iter()
            .map(|item| TodoItem {
                author: item.author,
                snippet: item.snippet,
                reason: item.reason,
                message_id: item.message_id.unwrap_or_default(),
            })
            .collect(),
        generated_at: chrono::Local::now().format("%H:%M").to_string(),
    })
}

/// Fall back to the first sentence of the summary when a model omits the headline.
fn headline_from_summary(summary: &str) -> String {
    let first_line = summary.lines().next().unwrap_or_default();
    first_line
        .split_once(". ")
        .map_or(first_line, |(sentence, _)| sentence)
        .trim_end_matches('.')
        .to_string()
}

/// Extract JSON from text that may be wrapped in markdown code blocks.
pub fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        return &trimmed[start..=end];
    }
    trimmed
}

const RESPONSE_SCHEMA: &str = r#"{
  "headline": "One line, at most 70 characters, naming the main topic",
  "summary": "Brief 2-3 sentence summary of the conversation",
  "action_items": [
    {
      "author": "username who posted the message",
      "snippet": "relevant quote from the message",
      "reason": "why this needs attention",
      "message_id": "the [id] from the message line, e.g. m12 or 123456789"
    }
  ]
}"#;

#[derive(Deserialize)]
pub struct RawDigest {
    #[serde(default, alias = "title")]
    pub headline: String,
    #[serde(alias = "overview", alias = "description")]
    pub summary: String,
    #[serde(default, alias = "items", alias = "todos", alias = "actions")]
    pub action_items: Vec<RawTodoItem>,
}

#[derive(Deserialize)]
pub struct RawTodoItem {
    #[serde(default)]
    pub author: String,
    #[serde(default, alias = "quote", alias = "text", alias = "message")]
    pub snippet: String,
    #[serde(default, alias = "description", alias = "why")]
    pub reason: String,
    #[serde(default, alias = "id", alias = "msg_id")]
    pub message_id: Option<String>,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::ai::prompt::*;

    #[test]
    fn parse_valid_digest() {
        let json = r#"{
            "summary": "Discussion about Rust borrow checker improvements.",
            "action_items": [
                {
                    "author": "dave",
                    "snippet": "@ferris thoughts on the new edition?",
                    "reason": "Direct question to you",
                    "message_id": "m12"
                }
            ]
        }"#;

        let digest = parse_digest(json, "c1").unwrap();
        assert_eq!(digest.channel_id, "c1");
        assert!(digest.summary.contains("borrow checker"));
        assert_eq!(digest.todos.len(), 1);
        assert_eq!(digest.todos[0].author, "dave");
        assert_eq!(digest.todos[0].message_id, "m12");
    }

    #[test]
    fn parse_digest_in_code_block() {
        let text = "```json\n{\"summary\": \"test\", \"action_items\": []}\n```";
        let digest = parse_digest(text, "c1").unwrap();
        assert_eq!(digest.summary, "test");
        assert!(digest.todos.is_empty());
    }

    #[test]
    fn parse_digest_no_action_items() {
        let json = r#"{"summary": "Quiet channel, nothing notable."}"#;
        let digest = parse_digest(json, "c1").unwrap();
        assert_eq!(digest.summary, "Quiet channel, nothing notable.");
        assert!(digest.todos.is_empty());
    }

    #[test]
    fn headline_is_kept_when_the_model_provides_one() {
        let json = r#"{"headline": "Edition migration", "summary": "Long text. More."}"#;
        let digest = parse_digest(json, "c1").unwrap();
        assert_eq!(digest.headline, "Edition migration");
    }

    #[test]
    fn missing_headline_falls_back_to_first_sentence() {
        let json = r#"{"summary": "Borrow checker news. Then async closures."}"#;
        let digest = parse_digest(json, "c1").unwrap();
        assert_eq!(digest.headline, "Borrow checker news");
    }

    #[test]
    fn blank_headline_falls_back_to_single_sentence_summary() {
        let json = r#"{"headline": "  ", "summary": "Quiet channel."}"#;
        let digest = parse_digest(json, "c1").unwrap();
        assert_eq!(digest.headline, "Quiet channel");
    }

    #[test]
    fn schema_requires_every_top_level_field() {
        let schema = digest_json_schema();
        assert_eq!(
            schema["required"],
            serde_json::json!(["headline", "summary", "action_items"])
        );
        assert_eq!(schema["additionalProperties"], serde_json::json!(false));
    }

    #[test]
    fn extract_json_from_code_block() {
        assert_eq!(extract_json("```json\n{\"a\": 1}\n```"), "{\"a\": 1}");
        assert_eq!(extract_json("{\"a\": 1}"), "{\"a\": 1}");
        assert_eq!(extract_json("  {\"a\": 1}  "), "{\"a\": 1}");
    }

    #[test]
    fn format_messages_includes_authors() {
        let messages = vec![
            MessageRow {
                id: "1".into(),
                channel_id: "c1".into(),
                author: "alice".into(),
                author_avatar_url: None,
                content: "hello".into(),
                attachments: Vec::new(),
                timestamp: "10:00".into(),
                edited: false,
                is_continuation: false,
            },
            MessageRow {
                id: "2".into(),
                channel_id: "c1".into(),
                author: "alice".into(),
                author_avatar_url: None,
                content: "world".into(),
                attachments: Vec::new(),
                timestamp: "10:01".into(),
                edited: false,
                is_continuation: true,
            },
        ];

        let formatted = format_messages(&messages);
        assert!(formatted.contains("[1]"), "should include message id");
        assert!(formatted.contains("alice: hello"));
        assert!(formatted.contains("world"));
    }
}
