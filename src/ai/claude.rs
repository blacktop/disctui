use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use crate::ai::Summarizer;
use crate::ai::prompt;
use crate::model::{ChannelDigest, MessageRow};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-haiku-4-5";
const MAX_TOKENS: u32 = 4096;

pub struct ClaudeSummarizer {
    client: reqwest::Client,
    api_key: String,
}

impl ClaudeSummarizer {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }
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

        let request = ApiRequest {
            model: MODEL.into(),
            max_tokens: MAX_TOKENS,
            system: system_prompt,
            messages: vec![ApiMessage {
                role: "user".into(),
                content: format!("Here is the conversation:\n\n{conversation}"),
            }],
        };

        let response = self
            .client
            .post(API_URL)
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

        let api_response: ApiResponse =
            serde_json::from_str(&body).map_err(|e| eyre!("failed to parse response: {e}"))?;

        let text = api_response
            .content
            .iter()
            .find(|b| b.block_type == "text")
            .map_or("{}", |b| b.text.as_str());

        prompt::parse_digest(text, channel_id)
    }
}

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}
