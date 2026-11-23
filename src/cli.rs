use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::digest;

#[derive(Parser)]
#[command(name = "disctui", version)]
#[command(about = "Discord TUI with AI summaries and a headless daily digest")]
pub struct Cli {
    /// Path to a config file
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Write debug logs
    #[arg(short, long, global = true)]
    pub debug: bool,

    /// Open this channel when the TUI starts
    #[arg(long, value_name = "CHANNEL_ID")]
    pub open: Option<String>,

    /// Manage your account and server list only (supports passkeys in your browser)
    #[cfg(feature = "experimental-discord")]
    #[arg(long, conflicts_with = "open")]
    pub account: bool,

    /// Run the sign-in window for a running disctui and save the session
    #[cfg(all(target_os = "macos", feature = "experimental-discord"))]
    #[arg(long, hide = true, conflicts_with_all = ["account", "open"])]
    pub web_login_helper: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Summarize the most active channels without opening the TUI
    Digest(DigestArgs),
    /// List the channels of every server
    Channels(ChannelsArgs),
    /// Tell a running disctui to open a channel
    Open {
        /// Channel id, as printed by `disctui channels`
        channel_id: String,
    },
    /// Print the Rex actions (Lua) that add disctui pickers to the command palette
    Rex,
}

#[derive(Args)]
pub struct DigestArgs {
    /// How far back to look, e.g. 12h, 24h, 3d, 1w
    #[arg(long, default_value = "24h", value_parser = digest::parse_window)]
    pub since: Duration,

    /// Output format
    #[arg(long, value_enum, default_value_t = DigestFormat::Text)]
    pub format: DigestFormat,

    /// Only include servers whose name contains this text, or whose id matches
    /// (repeatable)
    #[arg(long = "guild", value_name = "NAME_OR_ID")]
    pub guilds: Vec<String>,

    /// Channels to include per server
    #[arg(long, default_value_t = digest::DEFAULT_TOP)]
    pub top: usize,

    /// Channels to include overall
    #[arg(long, default_value_t = digest::DEFAULT_LIMIT)]
    pub limit: usize,

    /// Write the digest to this file instead of stdout
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Rank channels and flag mentions without calling an AI backend
    #[arg(long)]
    pub no_ai: bool,

    /// Use the built-in sample data instead of Discord
    #[arg(long)]
    pub mock: bool,
}

#[derive(Args)]
pub struct ChannelsArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = ListFormat::Text)]
    pub format: ListFormat,

    /// Use the built-in sample data instead of Discord
    #[arg(long)]
    pub mock: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DigestFormat {
    Text,
    Md,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ListFormat {
    Text,
    Json,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::cli::*;

    #[test]
    fn bare_invocation_starts_the_tui() {
        let cli = Cli::try_parse_from(["disctui"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.open.is_none());
    }

    #[test]
    fn digest_defaults_to_a_day_of_text() {
        let cli = Cli::try_parse_from(["disctui", "digest"]).unwrap();
        let Some(Command::Digest(args)) = cli.command else {
            unreachable!("digest subcommand should parse");
        };
        assert_eq!(args.since, Duration::from_hours(24));
        assert_eq!(args.format, DigestFormat::Text);
        assert_eq!((args.top, args.limit), (3, 15));
        assert!(!args.no_ai);
    }

    #[test]
    fn digest_accepts_repeated_guilds_and_formats() {
        let cli = Cli::try_parse_from([
            "disctui", "digest", "--since", "12h", "--format", "json", "--guild", "Tokio",
            "--guild", "rust", "--debug",
        ])
        .unwrap();
        let Some(Command::Digest(args)) = cli.command else {
            unreachable!("digest subcommand should parse");
        };
        assert_eq!(args.since, Duration::from_hours(12));
        assert_eq!(args.format, DigestFormat::Json);
        assert_eq!(args.guilds, vec!["Tokio", "rust"]);
        assert!(cli.debug);
    }

    #[test]
    fn bad_window_is_rejected_with_a_hint() {
        let err = Cli::try_parse_from(["disctui", "digest", "--since", "soon"])
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("e.g. 24h"), "{err}");
    }

    #[test]
    fn open_needs_a_channel_id() {
        assert!(Cli::try_parse_from(["disctui", "open"]).is_err());
        let cli = Cli::try_parse_from(["disctui", "open", "123"]).unwrap();
        let Some(Command::Open { channel_id }) = cli.command else {
            unreachable!("open subcommand should parse");
        };
        assert_eq!(channel_id, "123");
    }
}
