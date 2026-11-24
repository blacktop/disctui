//! Commands that run without the TUI: `digest`, `channels`, and `open`.

use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::Serialize;

use crate::action::Action;
use crate::app::App;
use crate::cli::{ChannelsArgs, DigestArgs, DigestFormat, ListFormat};
use crate::config::{AppConfig, NO_AI_BACKEND_HELP};
use crate::control;
use crate::digest::{self, DigestOptions, DigestReport, HistorySource, PreparedDigest};
use crate::status::{Report, Reporter, State};
use crate::transport::mock;

enum Source {
    Mock,
    #[cfg(feature = "experimental-discord")]
    Discord(live::Session),
}

#[cfg_attr(
    not(feature = "experimental-discord"),
    expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "only the Discord source awaits; the sample-data source is synchronous"
    )
)]
impl Source {
    async fn connect(use_mock: bool) -> Result<(Self, App)> {
        #[cfg(feature = "experimental-discord")]
        if !use_mock {
            let (session, snapshot) = live::connect().await?;
            return Ok((Self::Discord(session), snapshot));
        }
        #[cfg(not(feature = "experimental-discord"))]
        if !use_mock {
            tracing::warn!("built without Discord support; using sample data");
        }
        Ok((Self::Mock, mock_snapshot()))
    }

    fn history_source(&self) -> HistorySource {
        match self {
            Self::Mock => HistorySource::Mock,
            #[cfg(feature = "experimental-discord")]
            Self::Discord(session) => HistorySource::Discord(session.client.clone()),
        }
    }

    async fn close(self) {
        match self {
            Self::Mock => {}
            #[cfg(feature = "experimental-discord")]
            Self::Discord(session) => session.close().await,
        }
    }
}

/// Feed the fixtures through the app so unread roll-ups match the TUI.
fn mock_snapshot() -> App {
    let mut app = App::new();
    let _ = app.update(Action::GuildsLoaded(mock::guilds()));
    let guild_ids: Vec<String> = app.guilds.iter().map(|guild| guild.id.clone()).collect();
    for guild_id in guild_ids {
        let channels = mock::channels(&guild_id);
        let _ = app.update(Action::ChannelsLoaded {
            guild_id: Some(guild_id),
            channels,
        });
    }
    app.current_username = mock::USER_NAME.into();
    app
}

#[cfg(feature = "experimental-discord")]
mod live {
    use std::collections::BTreeSet;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use color_eyre::eyre::{Result, eyre};
    use tokio::sync::mpsc;
    use zeroize::Zeroize;

    use crate::action::Action;
    use crate::app::App;
    use crate::auth;
    use crate::model::DIRECT_MESSAGES_GUILD_ID;
    use crate::store::Store;
    use crate::transport::discord;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(45);
    const ACTION_CHANNEL_CAPACITY: usize = 512;

    pub struct Session {
        pub(crate) client: Arc<diself::Client>,
        handle: tokio::task::JoinHandle<()>,
    }

    pub async fn connect() -> Result<(Session, App)> {
        let mut token = auth::get_token().map_err(|err| {
            eyre!("no Discord token ({err}). Run `disctui` once to sign in, or set DISCTUI_TOKEN")
        })?;
        let (tx, mut rx) = mpsc::channel::<Action>(ACTION_CHANNEL_CAPACITY);
        let connected = discord::connect(token.clone(), tx);
        token.zeroize();
        let (client, handle) = connected?;
        let session = Session { client, handle };

        let mut app = App::new();
        match Store::open() {
            Ok(store) => app.store = Some(Rc::new(store)),
            Err(err) => tracing::warn!("digest runs without local read state: {err}"),
        }

        let ready = tokio::time::timeout(CONNECT_TIMEOUT, async {
            let missing = wait_until_ready(&mut app, &mut rx).await?;
            for id in missing {
                let channels = diself::ChannelsManager
                    .guild_channels(session.client.http(), &id)
                    .await
                    .map_err(|_| eyre!("could not load channels for guild {id}"))?;
                let _ = app.update(Action::ChannelsLoaded {
                    guild_id: Some(id),
                    channels: discord::channels_to_summaries(&channels, session.client.cache()),
                });
            }
            Ok(())
        });
        let ready = ready.await;
        rx.close();
        match ready {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                session.close().await;
                return Err(err);
            }
            Err(_) => {
                session.close().await;
                return Err(eyre!(
                    "Discord did not finish connecting within {}s",
                    CONNECT_TIMEOUT.as_secs()
                ));
            }
        }

        session.load_direct_messages(&mut app).await;
        Ok((session, app))
    }

    /// Give subscriptions a chance to supply channel lists before fetching the rest.
    async fn wait_until_ready(
        app: &mut App,
        rx: &mut mpsc::Receiver<Action>,
    ) -> Result<BTreeSet<String>> {
        let mut connected = false;
        let mut has_servers = false;
        let mut missing = BTreeSet::new();
        let mut subscription_deadline = None;
        loop {
            let action = if connected && has_servers {
                if missing.is_empty() {
                    break;
                }
                let deadline = *subscription_deadline
                    .get_or_insert_with(|| tokio::time::Instant::now() + Duration::from_secs(2));
                let Ok(action) = tokio::time::timeout_at(deadline, rx.recv()).await else {
                    break;
                };
                action
            } else {
                rx.recv().await
            }
            .ok_or_else(|| eyre!("Discord connection closed before it was ready"))?;
            match &action {
                Action::TransportConnected { .. } => connected = true,
                Action::ReadyData {
                    guilds,
                    guild_channels,
                    ..
                } => {
                    has_servers = true;
                    missing = guilds
                        .iter()
                        .filter(|guild| {
                            guild.id != DIRECT_MESSAGES_GUILD_ID
                                && guild_channels.get(&guild.id).is_none_or(Vec::is_empty)
                        })
                        .map(|guild| guild.id.clone())
                        .collect();
                }
                Action::GuildAvailable { guild, channels } if !channels.is_empty() => {
                    missing.remove(&guild.id);
                }
                Action::TransportSessionExpired => {
                    return Err(eyre!("Message session expired. Sign in again."));
                }
                Action::TransportDisconnected(reason) => {
                    return Err(eyre!("Discord disconnected: {reason}"));
                }
                _ => {}
            }
            let _ = app.update(action);
        }
        Ok(missing)
    }

    impl Session {
        async fn load_direct_messages(&self, app: &mut App) {
            let manager = diself::ChannelsManager;
            match manager.dm_channels(self.client.http()).await {
                Ok(channels) => {
                    let _ = app.update(Action::ChannelsLoaded {
                        guild_id: Some(DIRECT_MESSAGES_GUILD_ID.to_string()),
                        channels: discord::channels_to_summaries(&channels, self.client.cache()),
                    });
                }
                Err(err) => tracing::warn!("failed to load direct messages: {err}"),
            }
        }

        pub async fn close(self) {
            self.client.shutdown();
            if let Err(err) = self.handle.await {
                tracing::warn!("discord task join failed: {err}");
            }
        }
    }
    #[cfg(test)]
    #[expect(clippy::unwrap_used, reason = "fixture failures should fail the test")]
    mod tests {
        use crate::{action::Action, app::App, headless::live::wait_until_ready, transport::mock};

        #[tokio::test]
        async fn ready_waits_for_a_later_guild_channel_list() {
            let (tx, mut rx) = tokio::sync::mpsc::channel(4);
            let guild = mock::guilds()
                .into_iter()
                .find(|g| !mock::channels(&g.id).is_empty())
                .unwrap();
            let channels = mock::channels(&guild.id);
            let id = guild.id.clone();
            for action in [
                Action::TransportConnected {
                    username: "reader".into(),
                },
                Action::ReadyData {
                    guilds: vec![guild.clone()],
                    guild_channels: std::collections::HashMap::new(),
                    guild_mute_settings: std::collections::HashMap::new(),
                },
                Action::GuildAvailable { guild, channels },
            ] {
                tx.try_send(action).unwrap();
            }
            drop(tx);
            let mut app = App::new();
            assert!(
                wait_until_ready(&mut app, &mut rx)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                app.guild_channels()
                    .get(&id)
                    .is_some_and(|rows| !rows.is_empty())
            );
        }
    }
}

/// Where a digest goes.
#[derive(Debug, PartialEq, Eq)]
enum Destination {
    Stdout,
    File(PathBuf),
}

impl Destination {
    /// `--out` wins over the configured file, and `-` prints either way.
    fn resolve(flag: Option<PathBuf>, configured: Option<PathBuf>) -> Self {
        let Some(path) = flag.or(configured) else {
            return Self::Stdout;
        };
        if path.as_os_str() == "-" {
            Self::Stdout
        } else {
            Self::File(path)
        }
    }

    /// `--format` wins; otherwise a file's extension picks Markdown or JSON.
    fn format(&self, flag: Option<DigestFormat>) -> DigestFormat {
        if let Some(format) = flag {
            return format;
        }
        let extension = match self {
            Self::Stdout => None,
            Self::File(path) => path.extension().and_then(|ext| ext.to_str()),
        };
        match extension {
            Some("md" | "markdown") => DigestFormat::Md,
            Some("json") => DigestFormat::Json,
            Some(_) | None => DigestFormat::Text,
        }
    }
}

fn render(report: &DigestReport, format: DigestFormat, color: bool) -> Result<String> {
    Ok(match format {
        DigestFormat::Text => digest::render_text(report, color),
        DigestFormat::Md => digest::render_markdown(report),
        DigestFormat::Json => {
            let mut json = serde_json::to_string_pretty(report)?;
            json.push('\n');
            json
        }
    })
}

fn outcome_line(report: &DigestReport) -> String {
    let channels = report.channel_count();
    let servers = report.servers.len();
    let needs = report.needs_you.len();
    format!(
        "{} across {}, {} need{} you",
        digest::plural(channels, "channel"),
        digest::plural(servers, "server"),
        digest::plural(needs, "item"),
        if needs == 1 { "s" } else { "" },
    )
}

fn color_enabled() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn write_stdout(text: &str) -> Result<()> {
    io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .wrap_err("failed to write to stdout")
}

pub async fn run_digest(args: DigestArgs, config: &AppConfig) -> Result<()> {
    let mut status = Reporter::stderr();
    let result = digest_command(args, config, &mut status).await;
    match &result {
        Ok(outcome) => status.report(Report::new(State::Done, outcome.as_str())),
        Err(err) => status.report(Report::new(State::Error, err.to_string())),
    }
    result.map(|_| ())
}

async fn digest_command(
    args: DigestArgs,
    config: &AppConfig,
    status: &mut Reporter<io::Stderr>,
) -> Result<String> {
    let backend = if args.no_ai {
        None
    } else {
        config.summarizer_backend()
    };
    if backend.is_none() && !args.no_ai {
        tracing::warn!("digest: {NO_AI_BACKEND_HELP}");
    }

    status.report(Report::new(State::Working, "Connecting to Discord"));
    let (source, snapshot) = Source::connect(args.mock).await?;

    // An explicit --guild replaces the configured whitelist for this run.
    let include = if args.guilds.is_empty() {
        config.digest_include()
    } else {
        Vec::new()
    };
    let options = DigestOptions {
        window: args.since,
        guild_filters: args.guilds,
        include,
        top_per_server: args.top,
        max_channels: args.limit,
    };
    let request = PreparedDigest::new(
        &snapshot.guilds,
        snapshot.guild_channels(),
        &options,
        snapshot.current_username.clone(),
    );
    drop(snapshot);
    let report = request
        .generate(
            source.history_source(),
            backend.as_ref(),
            |index, total, label| {
                status.report(
                    Report::new(State::Working, format!("Reading {label}"))
                        .with_progress(index, total),
                );
            },
        )
        .await;
    source.close().await;

    let outcome = outcome_line(&report);

    // Sample data never replaces a saved report.
    let configured = if args.mock { None } else { config.digest_out() };
    let destination = Destination::resolve(args.out, configured);
    let format = destination.format(args.format);
    match destination {
        Destination::File(path) => {
            let rendered = render(&report, format, false)?;
            if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::write(&path, rendered)
                .wrap_err_with(|| format!("failed to write {}", path.display()))?;
            write_stdout(&format!(
                "Wrote {}: {outcome}.\n",
                digest::terminal_field(&path.to_string_lossy())
            ))?;
        }
        Destination::Stdout => write_stdout(&render(&report, format, color_enabled())?)?,
    }
    Ok(outcome)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ChannelRow {
    id: String,
    name: String,
    kind: &'static str,
    guild: String,
    guild_id: String,
    category: Option<String>,
    unread: bool,
    mentions: u32,
}

/// Every channel a person could open, in sidebar order.
fn channel_rows(snapshot: &App) -> Vec<ChannelRow> {
    let mut rows = Vec::new();
    for guild in &snapshot.guilds {
        let channels = snapshot
            .guild_channels()
            .get(&guild.id)
            .map_or_default(Vec::as_slice);
        for channel in channels {
            if !channel.kind.is_selectable() || snapshot.is_closed_direct_message(channel) {
                continue;
            }
            let category = channel.parent_id.as_ref().and_then(|parent_id| {
                channels
                    .iter()
                    .find(|parent| parent.id == *parent_id)
                    .map(|parent| parent.name.clone())
            });
            rows.push(ChannelRow {
                id: channel.id.clone(),
                name: channel.name.clone(),
                kind: channel.kind.label(),
                guild: guild.name.clone(),
                guild_id: guild.id.clone(),
                category,
                unread: channel.shows_unread_in_guild_rollup(guild.muted),
                mentions: channel.unread_count,
            });
        }
    }
    rows
}

fn render_channel_rows(rows: &[ChannelRow]) -> String {
    let mut out = String::new();
    for row in rows {
        let marker = if row.kind == "dm" { "@" } else { "#" };
        let unread = if row.unread { "*" } else { " " };
        let _ = writeln!(
            out,
            "{unread} {}\t{marker}{}\t{}",
            digest::terminal_field(&row.id),
            digest::terminal_field(&row.name),
            digest::terminal_field(&row.guild)
        );
    }
    out
}

pub async fn run_channels(args: ChannelsArgs) -> Result<()> {
    let (source, snapshot) = Source::connect(args.mock).await?;
    source.close().await;

    let rows = channel_rows(&snapshot);
    let rendered = match args.format {
        ListFormat::Text => render_channel_rows(&rows),
        ListFormat::Json => {
            let mut json = serde_json::to_string_pretty(&rows)?;
            json.push('\n');
            json
        }
    };
    write_stdout(&rendered)
}

/// Rex actions, embedded so they always match this binary's CLI.
const REX_ACTIONS: &str = include_str!("../rex/disctui.lua");

pub fn print_rex_actions() -> Result<()> {
    write_stdout(REX_ACTIONS)
}

pub async fn run_open(channel_id: String) -> Result<()> {
    if channel_id.trim().is_empty() {
        return Err(eyre!("channel id is empty"));
    }
    let request = control::Request::Open { channel_id };
    control::send(&control::socket_path(), &request).await
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::headless::*;

    async fn mock_report(options: &DigestOptions) -> DigestReport {
        let (source, snapshot) = Source::connect(true).await.unwrap();
        PreparedDigest::new(
            &snapshot.guilds,
            snapshot.guild_channels(),
            options,
            snapshot.current_username.clone(),
        )
        .generate(source.history_source(), None, |_, _, _| {})
        .await
    }

    #[tokio::test]
    async fn mock_digest_ranks_unread_channels_and_lists_quiet_servers() {
        let report = mock_report(&DigestOptions::default()).await;

        let servers: Vec<&str> = report.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(servers, vec!["Rust Lang", "Tokio", "Direct Messages"]);
        assert_eq!(report.servers[0].channels[0].name, "#general");
        assert_eq!(report.servers[0].channels[0].messages, 13);
        assert_eq!(report.quiet, vec!["ratatui", "Nix/NixOS"]);
    }

    #[tokio::test]
    async fn mock_digest_flags_the_mention_and_unanswered_dms() {
        let report = mock_report(&DigestOptions::default()).await;

        let flagged: Vec<(&str, &str)> = report
            .needs_you
            .iter()
            .map(|item| (item.channel.as_str(), item.author.as_str()))
            .collect();
        assert!(flagged.contains(&("#general", "dave")), "{flagged:?}");
        assert!(flagged.contains(&("@alice", "alice")), "{flagged:?}");
        assert!(flagged.contains(&("@incident-room", "sre")), "{flagged:?}");
    }

    #[tokio::test]
    async fn mock_digest_honors_server_filter() {
        let filtered = DigestOptions {
            guild_filters: vec!["tokio".into()],
            ..DigestOptions::default()
        };
        let report = mock_report(&filtered).await;

        assert_eq!(report.servers.len(), 1);
        assert_eq!(report.servers[0].name, "Tokio");
        assert_eq!(report.quiet, Vec::<String>::new());
    }

    #[test]
    fn channel_rows_skip_categories_and_name_their_parent() {
        let rows = channel_rows(&mock_snapshot());

        assert_eq!(rows.len(), 17);
        assert!(rows.iter().all(|row| row.kind != "category"));
        let general = rows.iter().find(|row| row.id == "c1").unwrap();
        assert_eq!(general.guild, "Rust Lang");
        assert_eq!(general.category.as_deref(), Some("The Basics"));
        assert!(general.unread);
        let dm = rows.iter().find(|row| row.id == "dm1").unwrap();
        assert_eq!(dm.kind, "dm");
        assert_eq!(dm.category, None);
    }

    #[test]
    fn channel_picker_omits_a_locally_closed_dm() {
        let mut app = mock_snapshot();
        let store = std::rc::Rc::new(crate::store::Store::open_in_memory());
        store.close_direct_message("dm1");
        app.store = Some(store);
        app.update(Action::GuildsLoaded(mock::guilds()));
        let rows = channel_rows(&app);
        assert!(rows.iter().any(|row| row.id == "dm2"));
        assert!(!rows.iter().any(|row| row.id == "dm1"));
    }

    #[test]
    fn channel_text_listing_is_tab_separated() {
        let rows = channel_rows(&mock_snapshot());
        let text = render_channel_rows(&rows);
        assert!(text.contains("* c1\t#general\tRust Lang\n"));
        assert!(text.contains("  c10\t#general\tratatui\n"));
    }

    #[test]
    fn plain_channels_preserve_one_record_per_channel() {
        let mut rows = channel_rows(&mock_snapshot());
        rows.truncate(1);
        let row = rows.first_mut().unwrap();
        row.id = "c\t1".into();
        row.name = "line\nname".into();
        row.guild = "server\x1b\u{009b}".into();
        let output = render_channel_rows(&rows);
        assert_eq!(output.lines().count(), 1);
        assert_eq!(output.matches('\t').count(), 2);
        assert!(!output.contains(['\x1b', '\u{009b}']));
    }

    #[test]
    fn outcome_line_handles_singular_and_plural() {
        let mut report = DigestReport {
            generated_at: String::new(),
            window: String::new(),
            model: None,
            needs_you: Vec::new(),
            servers: Vec::new(),
            quiet: Vec::new(),
        };
        assert_eq!(
            outcome_line(&report),
            "0 channels across 0 servers, 0 items need you"
        );
        report.needs_you.push(digest::NeedsYou {
            guild: String::new(),
            channel: String::new(),
            channel_id: String::new(),
            author: String::new(),
            time: String::new(),
            snippet: String::new(),
            reason: String::new(),
            message_id: String::new(),
            mention: false,
        });
        assert!(outcome_line(&report).ends_with("1 item needs you"));
    }

    #[test]
    fn digest_goes_to_the_flag_then_the_config_then_stdout() {
        let file = |path: &str| Some(PathBuf::from(path));

        assert_eq!(Destination::resolve(None, None), Destination::Stdout);
        assert_eq!(
            Destination::resolve(None, file("saved.md")),
            Destination::File("saved.md".into())
        );
        assert_eq!(
            Destination::resolve(file("once.md"), file("saved.md")),
            Destination::File("once.md".into())
        );
        assert_eq!(
            Destination::resolve(file("-"), file("saved.md")),
            Destination::Stdout
        );
    }

    #[test]
    fn digest_format_follows_the_flag_then_the_file_extension() {
        let format = |path: &str, flag| Destination::File(path.into()).format(flag);

        assert_eq!(format("today.md", None), DigestFormat::Md);
        assert_eq!(format("today.json", None), DigestFormat::Json);
        assert_eq!(format("today.txt", None), DigestFormat::Text);
        assert_eq!(format("today", None), DigestFormat::Text);
        assert_eq!(
            format("today.md", Some(DigestFormat::Json)),
            DigestFormat::Json
        );
        assert_eq!(Destination::Stdout.format(None), DigestFormat::Text);
    }

    #[test]
    fn rex_actions_call_subcommands_and_flags_that_exist() {
        use clap::Parser;

        use crate::cli::Cli;

        for action in ["disctui_jump", "disctui_inbox", "disctui_digest"] {
            assert!(REX_ACTIONS.contains(action), "missing action {action}");
        }
        // The exact invocations the Lua builds must parse.
        for argv in [
            vec!["disctui", "channels", "--format", "json", "--mock"],
            vec![
                "disctui", "digest", "--no-ai", "--format", "json", "--out", "-", "--since", "24h",
                "--limit", "10",
            ],
            vec![
                "disctui", "digest", "--out", "-", "--since", "24h", "--guild", "@me", "--guild",
                "g1",
            ],
            vec!["disctui", "open", "123"],
            vec!["disctui", "--open", "123"],
        ] {
            assert!(Cli::try_parse_from(&argv).is_ok(), "{argv:?} should parse");
        }
        // Lua 5.1 has no \u{...} escapes; non-ASCII must be literal.
        assert!(!REX_ACTIONS.contains("\\u{"));
    }

    #[tokio::test]
    async fn open_rejects_an_empty_channel_id() {
        assert!(run_open("  ".into()).await.is_err());
    }
}
