use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

use crate::ai::SummarizerBackend;
use crate::ai::{claude, local};
use crate::digest::Include;

pub const NO_AI_BACKEND_HELP: &str =
    "No AI backend configured. Set ANTHROPIC_API_KEY, or ai_backend = \"local\" with ai_base_url";

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub tick_rate_ms: u64,
    pub mouse: bool,
    pub ai_backend: String,
    pub ai_base_url: Option<String>,
    pub ai_model: Option<String>,
    /// Bearer token for the OpenAI-compatible endpoint. Local servers need none.
    pub ai_api_key: Option<String>,
    /// Digest whitelist: `Server`, or `Server #channel` for one channel.
    /// Empty means every server.
    pub digest_include: Vec<String>,
    /// File `disctui digest` writes when `--out` is not given.
    pub digest_out: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tick_rate_ms: 250,
            mouse: true,
            ai_backend: "auto".into(),
            ai_base_url: None,
            ai_model: None,
            ai_api_key: None,
            digest_include: Vec::new(),
            digest_out: None,
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

        // No key separator: `DISCTUI_AI_BACKEND` must map to the flat `ai_backend` key.
        builder = builder.add_source(config::Environment::with_prefix("DISCTUI").try_parsing(true));

        // Scalar inference in config::Environment runs before list splitting, even for
        // numeric guild IDs. Preserve the original text for this one list setting.
        let include = std::env::var("DISCTUI_DIGEST_INCLUDE").ok().map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
        let cfg = builder
            .set_override_option("digest_include", include)?
            .build()
            .wrap_err("failed to build configuration")?
            .try_deserialize()
            .wrap_err("failed to deserialize configuration")?;

        Ok(cfg)
    }

    /// The digest whitelist, with blank entries dropped.
    pub fn digest_include(&self) -> Vec<Include> {
        self.digest_include
            .iter()
            .filter_map(|entry| Include::parse(entry))
            .collect()
    }

    /// The configured digest file, with a leading `~` expanded to the home directory.
    pub fn digest_out(&self) -> Option<PathBuf> {
        let path = self.digest_out.as_deref()?;
        if path.as_os_str().is_empty() {
            return None;
        }
        Some(expand_home(path, dirs::home_dir().as_deref()))
    }

    pub fn tick_rate(&self) -> Duration {
        Duration::from_millis(self.tick_rate_ms)
    }

    /// Resolve which AI summarizer backend to use.
    ///
    /// - `claude`: Claude API, needs `ANTHROPIC_API_KEY`
    /// - `local`: OpenAI-compatible endpoint at `ai_base_url`
    /// - `none`: summaries disabled
    /// - `auto` (default): Claude when `ANTHROPIC_API_KEY` is set, otherwise the
    ///   OpenAI-compatible endpoint when `ai_base_url` is configured
    pub fn summarizer_backend(&self) -> Option<SummarizerBackend> {
        self.resolve_backend(anthropic_api_key())
    }

    fn resolve_backend(&self, anthropic_key: Option<String>) -> Option<SummarizerBackend> {
        match self.ai_backend.as_str() {
            "claude" => anthropic_key.map(|api_key| self.claude_backend(api_key)),
            "local" => Some(
                self.local_backend(
                    self.ai_base_url
                        .clone()
                        .unwrap_or_else(|| local::DEFAULT_BASE_URL.into()),
                ),
            ),
            "none" => None,
            _ => anthropic_key
                .map(|api_key| self.claude_backend(api_key))
                .or_else(|| {
                    self.ai_base_url
                        .clone()
                        .map(|base_url| self.local_backend(base_url))
                }),
        }
    }

    fn claude_backend(&self, api_key: String) -> SummarizerBackend {
        SummarizerBackend::Claude {
            api_key,
            model: self
                .ai_model
                .clone()
                .unwrap_or_else(|| claude::DEFAULT_MODEL.into()),
        }
    }

    fn local_backend(&self, base_url: String) -> SummarizerBackend {
        SummarizerBackend::Local {
            base_url,
            model: self
                .ai_model
                .clone()
                .unwrap_or_else(|| local::DEFAULT_MODEL.into()),
            api_key: self.ai_api_key.clone().filter(|key| !key.is_empty()),
        }
    }
}

/// The shell expands `~` on the command line; nothing does for a config value.
fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    let (Ok(rest), Some(home)) = (path.strip_prefix("~"), home) else {
        return path.to_path_buf();
    };
    home.join(rest)
}

fn anthropic_api_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "a wrong backend variant should fail the test loudly"
)]
mod tests {
    use crate::config::*;

    #[test]
    fn digest_whitelist_is_empty_by_default_and_drops_blank_entries() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.digest_include(), Vec::<Include>::new());

        cfg.digest_include = vec!["Tokio #changelog".into(), "  ".into()];
        assert_eq!(
            cfg.digest_include(),
            Vec::from_iter(Include::parse("tokio #changelog"))
        );
    }

    #[test]
    fn digest_out_is_unset_by_default_and_ignores_an_empty_path() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.digest_out(), None);

        cfg.digest_out = Some(PathBuf::new());
        assert_eq!(cfg.digest_out(), None);

        cfg.digest_out = Some("/tmp/digests/today.md".into());
        assert_eq!(
            cfg.digest_out(),
            Some(PathBuf::from("/tmp/digests/today.md"))
        );
    }

    #[test]
    fn digest_out_is_read_from_the_config_file() {
        let source = config::File::from_str(
            "digest_out = \"~/digests/today.md\"",
            config::FileFormat::Toml,
        );
        let cfg: AppConfig = config::Config::builder()
            .add_source(source)
            .build()
            .and_then(config::Config::try_deserialize)
            .unwrap_or_else(|err| panic!("config should load: {err}"));
        assert_eq!(cfg.digest_out, Some(PathBuf::from("~/digests/today.md")));
    }

    #[test]
    fn only_a_leading_tilde_of_its_own_expands_to_home() {
        let home = Some(Path::new("/Users/me"));
        let expand = |path: &str, home| expand_home(Path::new(path), home);

        assert_eq!(
            expand("~/digests/today.md", home),
            PathBuf::from("/Users/me/digests/today.md")
        );
        assert_eq!(expand("/tmp/a.md", home), PathBuf::from("/tmp/a.md"));
        assert_eq!(expand("~other/a.md", home), PathBuf::from("~other/a.md"));
        assert_eq!(expand("~/a.md", None), PathBuf::from("~/a.md"));
    }

    fn config(backend: &str) -> AppConfig {
        AppConfig {
            ai_backend: backend.into(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn auto_prefers_claude_when_a_key_is_present() {
        let mut cfg = config("auto");
        cfg.ai_base_url = Some("http://localhost:11434/v1".into());

        let backend = cfg.resolve_backend(Some("sk-test".into()));
        let Some(SummarizerBackend::Claude { model, .. }) = backend else {
            panic!("expected Claude backend, got {backend:?}");
        };
        assert_eq!(model, claude::DEFAULT_MODEL);
    }

    #[test]
    fn auto_falls_back_to_configured_endpoint() {
        let mut cfg = config("auto");
        cfg.ai_base_url = Some("http://localhost:11434/v1".into());
        cfg.ai_model = Some("llama3".into());

        let backend = cfg.resolve_backend(None);
        let Some(SummarizerBackend::Local {
            base_url,
            model,
            api_key,
        }) = backend
        else {
            panic!("expected local backend, got {backend:?}");
        };
        assert_eq!(base_url, "http://localhost:11434/v1");
        assert_eq!(model, "llama3");
        assert_eq!(api_key, None);
    }

    #[test]
    fn auto_without_key_or_endpoint_has_no_backend() {
        assert!(config("auto").resolve_backend(None).is_none());
    }

    #[test]
    fn none_disables_summaries_even_with_a_key() {
        assert!(config("none").resolve_backend(Some("sk".into())).is_none());
    }

    #[test]
    fn claude_without_a_key_has_no_backend() {
        assert!(config("claude").resolve_backend(None).is_none());
    }

    #[test]
    fn model_override_applies_to_claude() {
        let mut cfg = config("claude");
        cfg.ai_model = Some("claude-sonnet-5-5".into());

        let backend = cfg.resolve_backend(Some("sk".into()));
        let Some(SummarizerBackend::Claude { model, .. }) = backend else {
            panic!("expected Claude backend, got {backend:?}");
        };
        assert_eq!(model, "claude-sonnet-5-5");
    }

    #[test]
    fn local_uses_defaults_and_carries_the_api_key() {
        let mut cfg = config("local");
        cfg.ai_api_key = Some("token".into());

        let backend = cfg.resolve_backend(None);
        let Some(SummarizerBackend::Local {
            base_url,
            model,
            api_key,
        }) = backend
        else {
            panic!("expected local backend, got {backend:?}");
        };
        assert_eq!(base_url, local::DEFAULT_BASE_URL);
        assert_eq!(model, local::DEFAULT_MODEL);
        assert_eq!(api_key.as_deref(), Some("token"));
    }
}
