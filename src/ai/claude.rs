use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use crate::ai::Summarizer;
use crate::ai::prompt;
use crate::model::{ChannelDigest, MessageRow};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
/// Thinking counts toward the cap, so leave room for it ahead of the JSON answer.
const MAX_TOKENS: u32 = 16_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

pub struct ClaudeSummarizer {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl ClaudeSummarizer {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
        }
    }

    fn build_request(&self, system_prompt: String, conversation: &str) -> ApiRequest {
        ApiRequest {
            model: self.model.clone(),
            max_tokens: MAX_TOKENS,
            system: system_prompt,
            messages: vec![ApiMessage {
                role: "user".into(),
                content: format!("Here is the conversation:\n\n{conversation}"),
            }],
            output_config: OutputConfig {
                effort: "low",
                format: OutputFormat {
                    format_type: "json_schema",
                    schema: prompt::digest_json_schema(),
                },
            },
        }
    }
}

/// Pull the JSON answer out of a Messages API response body.
fn extract_text(body: &str) -> Result<String> {
    let response: ApiResponse =
        serde_json::from_str(body).map_err(|e| eyre!("failed to parse Claude response: {e}"))?;

    match response.stop_reason.as_deref() {
        Some("refusal") => return Err(eyre!("Claude declined to summarize this conversation")),
        Some("max_tokens") => {
            return Err(eyre!(
                "Claude ran out of output tokens before finishing the summary"
            ));
        }
        _ => {}
    }

    response
        .content
        .into_iter()
        .find(|block| block.block_type == "text")
        .map(|block| block.text)
        .ok_or_else(|| eyre!("Claude response had no text block"))
}

impl Summarizer for ClaudeSummarizer {
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

        let system_prompt = prompt::build_system_prompt(channel_name, user_name);
        let request = self.build_request(system_prompt, &conversation);

        let response = self
            .client
            .post(API_URL)
            .timeout(REQUEST_TIMEOUT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| eyre!("Claude API request failed: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| eyre!("failed to read Claude response: {e}"))?;

        if !status.is_success() {
            return Err(eyre!("Claude API returned {status}: {body}"));
        }

        let text = extract_text(&body)?;
        prompt::parse_digest(&text, channel_id)
    }
}

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
    output_config: OutputConfig,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OutputConfig {
    effort: &'static str,
    format: OutputFormat,
}

#[derive(Serialize)]
struct OutputFormat {
    #[serde(rename = "type")]
    format_type: &'static str,
    schema: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::ai::claude::*;

    #[test]
    fn request_asks_for_schema_constrained_json() {
        let summarizer = ClaudeSummarizer::new("key".into(), DEFAULT_MODEL.into());
        let request = summarizer.build_request("system".into(), "alice: hi");
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["model"], "claude-haiku-5-5");
        assert_eq!(json["output_config"]["format"]["type"], "json_schema");
        assert_eq!(
            json["output_config"]["format"]["schema"]["required"][0],
            "headline"
        );
        assert!(json.get("temperature").is_none());
    }

    #[test]
    fn text_block_is_found_after_thinking_blocks() {
        let body = r#"{
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": "", "signature": "sig"},
                {"type": "text", "text": "{\"summary\": \"ok\"}"}
            ]
        }"#;
        assert_eq!(extract_text(body).unwrap(), "{\"summary\": \"ok\"}");
    }

    #[test]
    fn refusal_is_reported_instead_of_parsed() {
        let body = r#"{"stop_reason": "refusal", "content": []}"#;
        let err = extract_text(body).unwrap_err().to_string();
        assert!(err.contains("declined"), "{err}");
    }

    #[test]
    fn truncated_output_is_an_error() {
        let body = r#"{"stop_reason": "max_tokens", "content": [{"type": "text", "text": "{"}]}"#;
        let err = extract_text(body).unwrap_err().to_string();
        assert!(err.contains("output tokens"), "{err}");
    }

    #[test]
    fn response_without_text_is_an_error() {
        let body = r#"{"stop_reason": "end_turn", "content": []}"#;
        assert!(extract_text(body).is_err());
    }
}
