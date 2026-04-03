use std::path::Path;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

use crate::ai::SummarizerBackend;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub tick_rate_ms: u64,
    pub mouse: bool,
    pub ai_backend: String,
    pub ai_base_url: Option<String>,
    pub ai_model: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tick_rate_ms: 250,
            mouse: false,
            ai_backend: "auto".into(),
            ai_base_url: None,
            ai_model: None,
        }
    }
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut builder = config::Config::builder();

        // Check platform config dir (~/Library/Application Support/ on macOS)
        if let Some(config_dir) = dirs::config_dir() {
            let default_path = config_dir.join("disctui").join("config.toml");
            if default_path.exists() {
                builder = builder.add_source(config::File::from(default_path));
            }
        }

        // Also check XDG-style ~/.config/disctui/ (common on macOS for CLI tools)
        if let Some(home) = dirs::home_dir() {
            let xdg_path = home.join(".config").join("disctui").join("config.toml");
            if xdg_path.exists() {
                builder = builder.add_source(config::File::from(xdg_path));
            }
        }

        if let Some(path) = path {
            builder = builder.add_source(config::File::from(path.to_path_buf()));
        }

        builder = builder.add_source(
            config::Environment::with_prefix("DISCTUI")
                .separator("_")
                .try_parsing(true),
        );

        let cfg = builder
            .build()
            .wrap_err("failed to build configuration")?
            .try_deserialize()
            .wrap_err("failed to deserialize configuration")?;

        Ok(cfg)
    }

    pub fn tick_rate(&self) -> Duration {
        Duration::from_millis(self.tick_rate_ms)
    }

    /// Resolve which AI summarizer backend to use.
    ///
    /// Resolution order for "auto" mode:
    /// 1. If `ANTHROPIC_API_KEY` is set -> Claude
    /// 2. If local LLM endpoint is reachable -> Local
    /// 3. None (no backend available)
    ///
    /// Explicit modes: "claude", "local", "none"
    pub fn summarizer_backend(&self) -> Option<SummarizerBackend> {
        match self.ai_backend.as_str() {
            "claude" => {
                let key = anthropic_api_key()?;
                Some(SummarizerBackend::Claude { api_key: key })
            }
            "local" => Some(SummarizerBackend::Local {
                base_url: self
                    .ai_base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:1234/v1".into()),
                model: self
                    .ai_model
                    .clone()
                    .unwrap_or_else(|| "google/gemma-4-26b-a4b".into()),
            }),
            "none" => None,
            // "auto" or anything else
            _ => {
                // Try Claude first
                if let Some(key) = anthropic_api_key() {
                    return Some(SummarizerBackend::Claude { api_key: key });
                }
                // Only fall back to local if a base URL is explicitly configured
                if let Some(base_url) = self.ai_base_url.clone() {
                    return Some(SummarizerBackend::Local {
                        base_url,
                        model: self
                            .ai_model
                            .clone()
                            .unwrap_or_else(|| "google/gemma-4-26b-a4b".into()),
                    });
                }
                // No backend available
                None
            }
        }
    }
}

fn anthropic_api_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}
