use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use crate::ai::Summarizer;
use crate::ai::prompt;
use crate::model::{ChannelDigest, MessageRow};

const DEFAULT_BASE_URL: &str = "http://localhost:1234/v1";
const DEFAULT_MODEL: &str = "google/gemma-4-26b-a4b";

pub struct LocalSummarizer {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl LocalSummarizer {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.into()),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.into()),
        }
    }

    fn build_request(&self, system_prompt: String, conversation: &str) -> ChatRequest {
        ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: system_prompt,
                },
                ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "Here is the conversation:\n\n{conversation}\n\n\
                         Respond with ONLY a JSON object, no other text."
                    ),
                },
            ],
            temperature: 0.3,
            max_tokens: 16384,
            response_format: serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "digest",
                    "strict": false,
                    "schema": {
                        "type": "object",
                        "properties": {
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
                                    "required": ["author", "snippet", "reason"]
                                }
                            }
                        },
                        "required": ["summary"]
                    }
                }
            }),
            reasoning_effort: Some("none".into()),
        }
    }

    fn extract_text(body: &str) -> Result<String> {
        let chat_response: ChatResponse =
            serde_json::from_str(body).map_err(|e| eyre!("failed to parse response: {e}"))?;

        let msg = chat_response.choices.first().map(|c| &c.message);

        // Try content first, then reasoning_content (models like Gemma 4
        // put chain-of-thought in reasoning_content and may leave content empty)
        let text = msg
            .map(|m| m.content.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                msg.and_then(|m| m.reasoning_content.as_deref())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("");

        if text.is_empty() {
            return Err(eyre!(
                "local LLM returned empty response (model may have hit context limit)"
            ));
        }

        Ok(text.to_string())
    }
}

impl Summarizer for LocalSummarizer {
    async fn summarize(
        &self,
        channel_name: &str,
        messages: &[MessageRow],
        user_name: &str,
    ) -> Result<ChannelDigest> {
        let conversation = prompt::format_messages(messages);
        let channel_id = messages
            .first()
            .map_or("unknown", |m| m.channel_id.as_str());

        tracing::info!(
            "summarizing {} messages ({} chars) via {}",
            messages.len(),
            conversation.len(),
            self.model,
        );

        let system_prompt = prompt::build_system_prompt(channel_name, user_name);
        let request = self.build_request(system_prompt, &conversation);
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                eyre!(
                    "local LLM request failed (is LM Studio running at {}?): {e}",
                    self.base_url
                )
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| eyre!("failed to read local LLM response: {e}"))?;

        if !status.is_success() {
            return Err(eyre!("local LLM returned {status}: {body}"));
        }

        tracing::info!(
            "local LLM response status={status}, body_len={}",
            body.len()
        );

        let text = Self::extract_text(&body)?;
        tracing::info!("local LLM response: {} chars", text.len());

        prompt::parse_digest(&text, channel_id).inspect_err(|_| {
            tracing::warn!("local LLM response failed to parse:\n{text}");
        })
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    response_format: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let s = LocalSummarizer::new(None, None);
        assert_eq!(s.base_url, "http://localhost:1234/v1");
        assert_eq!(s.model, "google/gemma-4-26b-a4b");
    }

    #[test]
    fn custom_config() {
        let s = LocalSummarizer::new(
            Some("http://localhost:11434/v1".into()),
            Some("llama3".into()),
        );
        assert_eq!(s.base_url, "http://localhost:11434/v1");
        assert_eq!(s.model, "llama3");
    }
}
