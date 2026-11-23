use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use crate::ai::Summarizer;
use crate::ai::prompt;
use crate::model::{ChannelDigest, MessageRow};

pub const DEFAULT_BASE_URL: &str = "http://localhost:1234/v1";
pub const DEFAULT_MODEL: &str = "google/gemma-4-26b-a4b";
/// Local models can take minutes on a long conversation.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Summarizer for any OpenAI-compatible chat completions endpoint: LM Studio,
/// Ollama, llama.cpp, or a hosted provider when an API key is set.
pub struct LocalSummarizer {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl LocalSummarizer {
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            model,
            api_key,
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
                    "schema": prompt::digest_json_schema()
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

        let mut http_request = self
            .client
            .post(&url)
            .timeout(REQUEST_TIMEOUT)
            .header("content-type", "application/json")
            .json(&request);
        if let Some(api_key) = &self.api_key {
            http_request = http_request.bearer_auth(api_key);
        }

        let response = http_request.send().await.map_err(|e| {
            eyre!(
                "LLM request failed (is a model server running at {}?): {e}",
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
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::ai::local::*;

    fn summarizer() -> LocalSummarizer {
        LocalSummarizer::new("http://localhost:11434/v1".into(), "llama3".into(), None)
    }

    #[test]
    fn request_uses_the_shared_digest_schema() {
        let request = summarizer().build_request("system".into(), "alice: hi");
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["model"], "llama3");
        assert_eq!(
            json["response_format"]["json_schema"]["schema"],
            prompt::digest_json_schema()
        );
    }

    #[test]
    fn content_is_preferred_over_reasoning() {
        let body = r#"{"choices": [{"message": {"content": "{}", "reasoning_content": "hmm"}}]}"#;
        assert_eq!(LocalSummarizer::extract_text(body).unwrap(), "{}");
    }

    #[test]
    fn reasoning_content_is_used_when_content_is_empty() {
        let body = r#"{"choices": [{"message": {"content": "", "reasoning_content": "{}"}}]}"#;
        assert_eq!(LocalSummarizer::extract_text(body).unwrap(), "{}");
    }

    #[test]
    fn empty_response_is_an_error() {
        let body = r#"{"choices": []}"#;
        assert!(LocalSummarizer::extract_text(body).is_err());
    }
}
