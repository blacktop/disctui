pub mod claude;
pub mod local;
mod prompt;

use color_eyre::eyre::Result;

use crate::ai::claude::ClaudeSummarizer;
use crate::ai::local::LocalSummarizer;
use crate::model::{ChannelDigest, MessageRow};

/// Trait for AI-powered conversation summarization.
pub trait Summarizer: Send + Sync {
    /// Summarize messages and identify items needing the user's attention.
    fn summarize(
        &self,
        channel_name: &str,
        messages: &[MessageRow],
        user_name: &str,
    ) -> impl std::future::Future<Output = Result<ChannelDigest>> + Send;
}

/// Maximum number of messages to include in a summary request.
const MAX_SUMMARY_MESSAGES: usize = 100;

/// Maximum total characters of message content to send.
const MAX_SUMMARY_CHARS: usize = 50_000;

/// Truncate and cap messages before sending to the AI backend.
fn prepare_messages_for_summary(messages: &[MessageRow]) -> Vec<MessageRow> {
    let mut total_chars = 0;
    let mut result = Vec::new();

    for msg in messages.iter().rev() {
        if result.len() >= MAX_SUMMARY_MESSAGES {
            break;
        }
        let content_len = msg.content.len();
        if total_chars + content_len > MAX_SUMMARY_CHARS {
            // Always include at least one message (truncate if oversized)
            if result.is_empty() {
                let mut truncated = msg.clone();
                truncated.content = msg.content.chars().take(MAX_SUMMARY_CHARS).collect();
                result.push(truncated);
            }
            break;
        }
        total_chars += content_len;
        result.push(msg.clone());
    }

    result.reverse();
    result
}

#[cfg(test)]
mod tests {
    use crate::ai::*;
    use crate::model::MessageRow;

    fn msg(id: &str, content: &str) -> MessageRow {
        MessageRow {
            id: id.into(),
            channel_id: "c1".into(),
            author: "user".into(),
            author_avatar_url: None,
            content: content.into(),
            attachments: Vec::new(),
            timestamp: "12:00".into(),
            edited: false,
            is_continuation: false,
        }
    }

    #[test]
    fn returns_chronological_order() {
        let messages = vec![msg("1", "first"), msg("2", "second"), msg("3", "third")];
        let result = prepare_messages_for_summary(&messages);
        assert_eq!(result[0].id, "1");
        assert_eq!(result[2].id, "3");
    }

    #[test]
    fn respects_message_limit() {
        let messages: Vec<_> = (0..200).map(|i| msg(&i.to_string(), "short")).collect();
        let result = prepare_messages_for_summary(&messages);
        assert!(result.len() <= MAX_SUMMARY_MESSAGES);
    }

    #[test]
    fn always_includes_at_least_one_oversized_message() {
        let big = "x".repeat(MAX_SUMMARY_CHARS + 1000);
        let messages = vec![msg("1", &big)];
        let result = prepare_messages_for_summary(&messages);
        assert_eq!(result.len(), 1);
        assert!(result[0].content.len() <= MAX_SUMMARY_CHARS);
    }

    #[test]
    fn empty_input_returns_empty() {
        let result = prepare_messages_for_summary(&[]);
        assert!(result.is_empty());
    }
}

/// Which AI backend to use for summarization.
#[derive(Debug, Clone)]
pub enum SummarizerBackend {
    /// Claude API (requires `ANTHROPIC_API_KEY`)
    Claude { api_key: String, model: String },
    /// OpenAI-compatible chat completions endpoint: a local server (LM Studio,
    /// Ollama) or a hosted one when `api_key` is set.
    Local {
        base_url: String,
        model: String,
        api_key: Option<String>,
    },
}

impl SummarizerBackend {
    /// Summarize one channel's messages with this backend.
    pub async fn summarize(
        &self,
        channel_name: &str,
        messages: &[MessageRow],
        user_name: &str,
    ) -> Result<ChannelDigest> {
        let prepared = prepare_messages_for_summary(messages);
        match self {
            Self::Claude { api_key, model } => {
                ClaudeSummarizer::new(api_key.clone(), model.clone())
                    .summarize(channel_name, &prepared, user_name)
                    .await
            }
            Self::Local {
                base_url,
                model,
                api_key,
            } => {
                LocalSummarizer::new(base_url.clone(), model.clone(), api_key.clone())
                    .summarize(channel_name, &prepared, user_name)
                    .await
            }
        }
    }

    /// Short name for status lines and digest headers.
    pub fn label(&self) -> String {
        match self {
            Self::Claude { model, .. } | Self::Local { model, .. } => model.clone(),
        }
    }
}
