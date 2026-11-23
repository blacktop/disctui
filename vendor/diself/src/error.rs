use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptchaInfo {
    pub captcha_key: Vec<String>,
    pub captcha_sitekey: String,
    pub captcha_service: String,
    pub captcha_session_id: Option<String>,
    pub captcha_rqdata: Option<String>,
    pub captcha_rqtoken: Option<String>,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Gateway connection error: {0}")]
    GatewayConnection(String),

    #[error("Gateway authentication failed (4004)")]
    GatewayAuthenticationFailed,

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid payload received")]
    InvalidPayload,

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Rate limited for {retry_after}s")]
    RateLimit { retry_after: f64 },

    #[error("Captcha required but no handler provided")]
    CaptchaRequired(CaptchaInfo),

    #[error("Captcha handler failed: {0}")]
    CaptchaHandlerFailed(String),

    #[error("Discord API error {status}: {body}")]
    Api { status: u16, body: String },
}

pub type Result<T> = std::result::Result<T, Error>;
