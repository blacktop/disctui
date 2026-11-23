mod action;
mod ai;
mod app;
#[cfg(feature = "experimental-discord")]
mod auth;
mod cli;
mod config;
mod control;
mod digest;
mod effect;
mod event;
mod headless;
mod logging;
mod model;
#[cfg(feature = "experimental-discord")]
mod oauth;
mod status;
mod store;
mod transport;
mod tui;
mod ui;
#[cfg(all(target_os = "macos", feature = "experimental-discord"))]
mod web_login;

use clap::Parser;
use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::time::interval;
#[cfg(feature = "experimental-discord")]
use zeroize::Zeroize;

use crate::action::Action;
use crate::ai::SummarizerBackend;
use crate::app::{App, InputMode};
#[cfg(feature = "experimental-discord")]
use crate::app::{BrowserPhase, ConnectionState};
use crate::cli::{Cli, Command};
use crate::config::AppConfig;
use crate::digest::{HistorySource, PreparedDigest};
use crate::effect::Effect;
#[cfg(not(feature = "experimental-discord"))]
use crate::model::LoadScope;
#[cfg(feature = "experimental-discord")]
use crate::model::{DIRECT_MESSAGES_GUILD_ID, LoadScope};
use crate::transport::mock;

const ACTION_CHANNEL_CAPACITY: usize = 512;

#[cfg(feature = "experimental-discord")]
struct DiscordState {
    session: std::time::Instant,
    client: std::sync::Arc<diself::Client>,
    handle: tokio::task::JoinHandle<()>,
}

/// What the run loop holds for a Discord build: the live connection, whether
/// any transport (Discord or sample data) has started, and the sign-in
/// attempts behind the sign-in prompt.
#[cfg(feature = "experimental-discord")]
#[derive(Default)]
struct DiscordRuntime {
    account_only: bool,
    state: Option<DiscordState>,
    transport_started: bool,
    /// Discord's own sign-in window, running as a child process.
    #[cfg(target_os = "macos")]
    web_sign_in: Option<tokio::task::JoinHandle<()>>,
    browser: Option<tokio::task::JoinHandle<Result<oauth::Outcome>>>,
    cancel_browser: Option<tokio::sync::oneshot::Sender<()>>,
}

#[cfg(feature = "experimental-discord")]
impl DiscordRuntime {
    fn start(&mut self, app: &mut App, action_tx: &mpsc::Sender<Action>, account: bool) {
        self.account_only = account;
        if account {
            self.start_browser(app, oauth::Operation::Restore);
        } else {
            start_transport(app, action_tx, self);
        }
    }

    fn start_browser(&mut self, app: &mut App, op: oauth::Operation) {
        if self.browser.is_some() {
            return;
        }
        let phase = match op {
            oauth::Operation::Restore => BrowserPhase::Restore,
            oauth::Operation::Authorize => BrowserPhase::Waiting,
            oauth::Operation::SignOut => {
                let Some(BrowserPhase::SignedIn(account)) = app.browser_phase() else {
                    return;
                };
                BrowserPhase::SigningOut(account.clone())
            }
        };
        app.set_browser_phase(phase);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cancel_browser = Some(tx);
        self.browser = Some(tokio::spawn(oauth::run(op, rx)));
    }

    fn cancel_browser(&mut self) {
        if let Some(cancel) = self.cancel_browser.take() {
            let _ = cancel.send(());
        }
    }

    async fn browser_result(&mut self) -> Result<oauth::Outcome> {
        let Some(task) = &mut self.browser else {
            return std::future::pending().await;
        };
        task.await?
    }

    fn finish_browser(&mut self, app: &mut App, result: Result<oauth::Outcome>) -> Vec<Effect> {
        self.browser = None;
        self.cancel_browser = None;
        let ready = if self.account_only {
            BrowserPhase::AccountReady
        } else {
            BrowserPhase::Ready
        };
        match result {
            Ok(oauth::Outcome::Account(name, guilds)) => {
                return app.show_browser_account(name, guilds);
            }
            Ok(oauth::Outcome::NeedsConsent) => {
                app.set_browser_phase(ready);
                app.set_sign_in_error(
                    "Server-list permission has changed. Press b to approve it in your browser."
                        .into(),
                );
            }
            Ok(oauth::Outcome::Ready) => {
                let previous_error = app.sign_in().and_then(|prompt| prompt.error.clone());
                app.set_browser_phase(ready);
                if let Some(error) = previous_error {
                    app.set_sign_in_error(error);
                }
            }
            Ok(oauth::Outcome::LocalSignOut) => {
                app.set_browser_phase(ready);
                app.set_sign_in_error("Signed out locally. To finish revoking access, open Authorized Apps in Discord.".into());
            }
            Err(err) => {
                let phase = if let Some(BrowserPhase::SigningOut(account)) = app.browser_phase() {
                    BrowserPhase::SignedIn(account.clone())
                } else {
                    ready
                };
                app.set_browser_phase(phase);
                app.set_sign_in_error(format!("{err:#}"));
            }
        }
        Vec::new()
    }

    /// Open Discord's sign-in window beside the terminal. One at a time:
    /// a second window would race the first for the same Keychain entry.
    #[cfg(target_os = "macos")]
    fn start_web_sign_in(&mut self, app: &mut App, tx: &mpsc::Sender<Action>) {
        if self.web_sign_in.is_some() {
            return;
        }
        self.cancel_browser();
        app.show_web_sign_in();
        self.web_sign_in = Some(tokio::spawn(web_login::run_sign_in(tx.clone())));
    }

    /// Drop the sign-in task, which kills the child and closes its window.
    #[cfg(target_os = "macos")]
    fn stop_web_sign_in(&mut self) {
        if let Some(attempt) = self.web_sign_in.take() {
            attempt.abort();
        }
    }
}

// Single-threaded: Rc<Store> is not Send, so we must not migrate futures across threads.
// A TUI has no need for multi-threaded work-stealing — all I/O is async via tokio.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();
    #[cfg(all(target_os = "macos", feature = "experimental-discord"))]
    if cli.web_login_helper {
        logging::init_headless(cli.debug);
        return web_login::helper().await;
    }
    let config = AppConfig::load(cli.config.as_deref())?;
    if let Some(command) = cli.command {
        logging::init_headless(cli.debug);
        return match command {
            Command::Digest(args) => headless::run_digest(args, &config).await,
            Command::Channels(args) => headless::run_channels(args).await,
            Command::Open { channel_id } => headless::run_open(channel_id).await,
            Command::Rex => headless::print_rex_actions(),
        };
    }

    logging::init(cli.debug)?;
    tracing::info!("disctui starting");
    tracing::info!(
        "ai_backend={}, ai_base_url={:?}",
        config.ai_backend,
        config.ai_base_url
    );

    let avatars = ui::media::AvatarStore::detect();
    tracing::info!("image protocol: {}", avatars.protocol_label());
    let store = std::rc::Rc::new(store::Store::open()?);

    let mut terminal = tui::init()?;
    let result = run(&mut terminal, &config, avatars, store, cli).await;
    if let Err(err) = &result {
        tracing::error!("run loop exited with error: {err:?}");
    }
    tui::restore()?;

    tracing::info!("disctui shutdown");
    result
}

async fn run(
    terminal: &mut tui::CrosstermTerminal,
    config: &AppConfig,
    avatars: ui::media::AvatarStore,
    store: std::rc::Rc<store::Store>,
    cli: Cli,
) -> Result<()> {
    let mut app = App::new_with_avatars(avatars);
    app.store = Some(store);
    app.digest_include = config.digest_include();
    let (action_tx, mut action_rx) = mpsc::channel::<Action>(ACTION_CHANNEL_CAPACITY);
    let _control_socket = match control::listen(control::socket_path(), action_tx.clone()).await {
        Ok(guard) => guard,
        Err(err) => {
            tracing::warn!("control socket unavailable: {err}");
            None
        }
    };
    if let Some(channel_id) = cli.open {
        let _ = action_tx.try_send(Action::OpenChannel(channel_id));
    }
    let mut program_status = status::Reporter::stdout();
    let mut term_events = EventStream::new();
    let mut tick = interval(config.tick_rate());
    let ai_backend = config.summarizer_backend();
    tick.tick().await;

    #[cfg(feature = "experimental-discord")]
    let mut discord = DiscordRuntime::default();
    #[cfg(feature = "experimental-discord")]
    discord.start(&mut app, &action_tx, cli.account);
    #[cfg(not(feature = "experimental-discord"))]
    {
        start_mock_transport(&action_tx, ai_backend.as_ref());
    }

    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;
        program_status.report(app.program_status());

        #[cfg(feature = "experimental-discord")]
        let browser_result = discord.browser_result();
        #[cfg(not(feature = "experimental-discord"))]
        let browser_result = std::future::pending::<()>();
        let action = tokio::select! {
            result = browser_result => {
                #[cfg(feature = "experimental-discord")]
                {
                    let effects = discord.finish_browser(&mut app, result);
                    dispatch_effects(effects, &action_tx, ai_backend.as_ref(), discord.state.as_ref(), app.connection_state == ConnectionState::MockTransport);
                }
                #[cfg(not(feature = "experimental-discord"))]
                let () = result;
                None
            }
            Some(Ok(evt)) = term_events.next() => map_runtime_event(&evt, &mut app),
            Some(action) = action_rx.recv() => Some(action),
            _ = tick.tick() => Some(Action::Tick),
        };

        if let Some(action) = action {
            process_action(
                action,
                &mut app,
                &action_tx,
                ai_backend.as_ref(),
                #[cfg(feature = "experimental-discord")]
                &mut discord,
            );
        }

        while let Ok(action) = action_rx.try_recv() {
            process_action(
                action,
                &mut app,
                &action_tx,
                ai_backend.as_ref(),
                #[cfg(feature = "experimental-discord")]
                &mut discord,
            );
        }

        if app.should_quit {
            app.save_on_quit();
            #[cfg(all(target_os = "macos", feature = "experimental-discord"))]
            discord.stop_web_sign_in();
            break;
        }
    }
    program_status.report(status::Report::new(status::State::Idle, ""));

    #[cfg(feature = "experimental-discord")]
    {
        discord.cancel_browser();
        if let Some(task) = discord.browser.take()
            && let Err(err) = task.await
        {
            tracing::warn!("browser sign-in task ended: {err}");
        }
        if let Some(ds) = discord.state {
            ds.client.shutdown();
            let _ = ds.handle.await;
        }
    }

    Ok(())
}

#[cfg(feature = "experimental-discord")]
fn start_transport(app: &mut App, tx: &mpsc::Sender<Action>, discord: &mut DiscordRuntime) {
    match auth::get_token() {
        Ok(token) => match connect_discord_with_token(&token, tx) {
            Ok(state) => {
                app.start_load_scope(LoadScope::StartupConnect);
                discord.state = Some(state);
                app.connection_state = ConnectionState::Connecting;
                discord.transport_started = true;
            }
            Err(err) => {
                tracing::error!("failed to connect to discord: {err}");
                discord.start_browser(app, oauth::Operation::Restore);
                app.set_sign_in_error(format!("Saved token failed to connect: {err}"));
            }
        },
        Err(err) => {
            tracing::info!("discord token unavailable at startup: {err}");
            discord.start_browser(app, oauth::Operation::Restore);
        }
    }
}

fn start_mock_transport(tx: &mpsc::Sender<Action>, ai_backend: Option<&SummarizerBackend>) {
    for effect in App::init_effects() {
        execute_effect_mock(effect, tx, ai_backend);
    }
}

fn map_runtime_event(evt: &Event, app: &mut App) -> Option<Action> {
    #[cfg(feature = "experimental-discord")]
    if app.has_sign_in() {
        return map_sign_in_event(evt, app);
    }

    let mapped = event::map_terminal_event(evt, app.input_mode, app.focus);
    if app.digest.visible {
        return mapped.filter(|action| {
            matches!(
                action,
                Action::Quit
                    | Action::Resize { .. }
                    | Action::MoveUp
                    | Action::MoveDown
                    | Action::ScrollUp(_)
                    | Action::ScrollDown(_)
                    | Action::JumpTop
                    | Action::JumpBottom
                    | Action::OpenSelected
                    | Action::RegenerateDigest
                    | Action::ShowMessages
                    | Action::ToggleHelp
                    | Action::ReconnectTransport
            )
        });
    }
    if mapped.is_none() && app.input_mode == InputMode::Insert {
        handle_insert_input(evt, app);
    }
    mapped
}

fn process_action(
    action: Action,
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    ai_backend: Option<&SummarizerBackend>,
    #[cfg(feature = "experimental-discord")] discord: &mut DiscordRuntime,
) {
    #[cfg(feature = "experimental-discord")]
    if let Action::MessageSessionRejected {
        session,
        channel_id,
        message,
    } = &action
    {
        if discord
            .state
            .as_ref()
            .is_some_and(|state| state.session == *session)
        {
            let _ = app.update(Action::LoadFailed {
                scope: LoadScope::History(channel_id.clone()),
                message: message.clone(),
            });
            let _ = app.update(Action::TransportSessionExpired);
            dispose_discord_state(&mut discord.state, true);
            discord.transport_started = false;
        }
        return;
    }

    #[cfg(feature = "experimental-discord")]
    if handle_sign_in_action(&action, app, tx, ai_backend, discord) {
        return;
    }

    #[cfg(feature = "experimental-discord")]
    if handle_reconnect_action(&action, app, tx, discord) {
        return;
    }

    #[cfg(feature = "experimental-discord")]
    if matches!(action, Action::TransportSessionExpired) {
        discord.transport_started = false;
    }
    #[cfg(feature = "experimental-discord")]
    let should_dispose_discord = matches!(
        action,
        Action::TransportDisconnected(_) | Action::TransportSessionExpired
    );
    let effects = app.update(action);
    #[cfg(feature = "experimental-discord")]
    if should_dispose_discord {
        dispose_discord_state(&mut discord.state, false);
    }
    #[cfg(feature = "experimental-discord")]
    let use_mock = app.connection_state == ConnectionState::MockTransport;
    dispatch_effects(
        effects,
        tx,
        ai_backend,
        #[cfg(feature = "experimental-discord")]
        discord.state.as_ref(),
        #[cfg(feature = "experimental-discord")]
        use_mock,
    );
}

#[cfg(feature = "experimental-discord")]
fn handle_reconnect_action(
    action: &Action,
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    discord: &mut DiscordRuntime,
) -> bool {
    let Action::ReconnectTransport = action else {
        return false;
    };

    match app.connection_state {
        ConnectionState::Disconnected
        | ConnectionState::SessionExpired
        | ConnectionState::MockTransport => {
            reconnect_discord(app, tx, discord);
        }
        ConnectionState::Connected => {
            let _ = app.update(Action::Error("Already connected".into()));
        }
        ConnectionState::Connecting | ConnectionState::Reconnecting => {
            let _ = app.update(Action::Error("Reconnect already in progress".into()));
        }
    }
    true
}

#[cfg(feature = "experimental-discord")]
fn reconnect_discord(app: &mut App, tx: &mpsc::Sender<Action>, discord: &mut DiscordRuntime) {
    let preserve_mock_transport = app.connection_state == ConnectionState::MockTransport;
    dispose_discord_state(&mut discord.state, true);

    let Ok(mut token) = auth::get_token() else {
        // Nothing saved yet: connecting means signing in.
        show_sign_in_after_reconnect(app, discord, preserve_mock_transport, None);
        return;
    };

    app.start_load_scope(LoadScope::StartupConnect);
    match connect_discord_with_token(&token, tx) {
        Ok(state) => {
            app.connection_state = ConnectionState::Reconnecting;
            discord.state = Some(state);
            discord.transport_started = true;
        }
        Err(err) => {
            app.finish_load_scope(&LoadScope::StartupConnect);
            show_sign_in_after_reconnect(
                app,
                discord,
                preserve_mock_transport,
                Some(format!("Reconnect failed: {err}")),
            );
        }
    }
    token.zeroize();
}

#[cfg(feature = "experimental-discord")]
fn show_sign_in_after_reconnect(
    app: &mut App,
    discord: &mut DiscordRuntime,
    preserve_mock_transport: bool,
    error: Option<String>,
) {
    if !preserve_mock_transport {
        discord.transport_started = false;
    }
    discord.start_browser(app, oauth::Operation::Restore);
    if let Some(error) = error {
        app.set_sign_in_error(error);
    }
}

#[cfg(feature = "experimental-discord")]
fn dispose_discord_state(discord: &mut Option<DiscordState>, shutdown: bool) {
    let Some(state) = discord.take() else {
        return;
    };

    if shutdown {
        state.client.shutdown();
    }

    tokio::spawn(async move {
        if let Err(err) = state.handle.await {
            tracing::warn!("discord task join failed: {err}");
        }
    });
}

fn dispatch_effects(
    effects: Vec<Effect>,
    tx: &mpsc::Sender<Action>,
    ai_backend: Option<&SummarizerBackend>,
    #[cfg(feature = "experimental-discord")] discord: Option<&DiscordState>,
    #[cfg(feature = "experimental-discord")] use_mock: bool,
) {
    for effect in effects {
        #[cfg(feature = "experimental-discord")]
        if let Some(ds) = discord {
            execute_effect_discord(effect, tx, ds, ai_backend);
        } else if use_mock {
            execute_effect_mock(effect, tx, ai_backend);
        } else {
            execute_effect_offline(effect, tx, ai_backend);
        }
        #[cfg(not(feature = "experimental-discord"))]
        execute_effect_mock(effect, tx, ai_backend);
    }
}

/// Effects of a live session that has no connection. Loads wait for a
/// reconnect; a summary needs only the messages already on screen.
#[cfg(feature = "experimental-discord")]
fn execute_effect_offline(
    effect: Effect,
    tx: &mpsc::Sender<Action>,
    ai_backend: Option<&SummarizerBackend>,
) {
    match effect {
        Effect::FetchAvatar { url } => execute_fetch_avatar(tx, &url),
        Effect::SummarizeChannel {
            channel_name,
            messages,
            user_name,
            ..
        } => {
            execute_summarize(tx, &channel_name, messages, &user_name, ai_backend);
        }
        Effect::SendMessage { .. } => {
            let _ = tx.try_send(Action::Error(
                "Not connected. The message was not sent".into(),
            ));
        }
        Effect::LoadGuilds
        | Effect::LoadChannels { .. }
        | Effect::LoadHistory { .. }
        | Effect::GenerateDigest { .. } => {}
    }
}

fn execute_effect_mock(
    effect: Effect,
    tx: &mpsc::Sender<Action>,
    ai_backend: Option<&SummarizerBackend>,
) {
    match effect {
        Effect::LoadGuilds => {
            let tx = tx.clone();
            let _ = tx.try_send(Action::LoadStarted(LoadScope::GuildBootstrap));
            let guilds = mock::guilds();
            tokio::spawn(async move {
                let _ = tx.send(Action::GuildsLoaded(guilds)).await;
            });
        }
        Effect::LoadChannels { guild_id } => {
            let tx = tx.clone();
            let _ = tx.try_send(Action::LoadStarted(LoadScope::ChannelList(
                guild_id.clone(),
            )));
            let channels = mock::channels(&guild_id);
            tokio::spawn(async move {
                let _ = tx
                    .send(Action::ChannelsLoaded {
                        guild_id: Some(guild_id),
                        channels,
                    })
                    .await;
            });
        }
        Effect::LoadHistory { channel_id } => {
            let tx = tx.clone();
            let _ = tx.try_send(Action::LoadStarted(LoadScope::History(channel_id.clone())));
            let messages = mock::messages(&channel_id);
            tokio::spawn(async move {
                let _ = tx
                    .send(Action::HistoryLoaded {
                        channel_id,
                        messages,
                        has_more: false,
                    })
                    .await;
            });
        }
        Effect::SendMessage { .. } => {} // already appended locally
        Effect::SummarizeChannel {
            channel_name,
            messages,
            user_name,
            ..
        } => {
            execute_summarize(tx, &channel_name, messages, &user_name, ai_backend);
        }
        Effect::GenerateDigest { job, request } => {
            execute_digest(tx, job, request, HistorySource::Mock, ai_backend);
        }
        Effect::FetchAvatar { url } => execute_fetch_avatar(tx, &url),
    }
}

#[cfg(feature = "experimental-discord")]
fn execute_effect_discord(
    effect: Effect,
    tx: &mpsc::Sender<Action>,
    ds: &DiscordState,
    ai_backend: Option<&SummarizerBackend>,
) {
    match effect {
        Effect::LoadGuilds => {
            // Guilds arrive via GUILD_CREATE events, not loaded explicitly
        }
        Effect::LoadChannels { guild_id } => {
            // Channels usually arrive via GUILD_CREATE and are cached in app.guild_channels.
            // This fallback uses REST if needed.
            let http = ds.client.http().clone();
            let cache = ds.client.cache().clone();
            let tx = tx.clone();
            let _ = tx.try_send(Action::LoadStarted(LoadScope::ChannelList(
                guild_id.clone(),
            )));
            tokio::spawn(async move {
                let action = if guild_id == DIRECT_MESSAGES_GUILD_ID {
                    let manager = diself::ChannelsManager;
                    match manager.dm_channels(&http).await {
                        Ok(channels) => Action::ChannelsLoaded {
                            guild_id: Some(guild_id),
                            channels: transport::discord::channels_to_summaries(&channels, &cache),
                        },
                        Err(e) => Action::LoadFailed {
                            scope: LoadScope::ChannelList(guild_id),
                            message: format!("failed to load direct messages: {e}"),
                        },
                    }
                } else {
                    let url = format!("https://discord.com/api/v10/guilds/{guild_id}/channels");
                    match http.get(&url).await {
                        Ok(value) => match serde_json::from_value::<Vec<diself::Channel>>(value) {
                            Ok(channels) => Action::ChannelsLoaded {
                                guild_id: Some(guild_id),
                                channels: transport::discord::channels_to_summaries(
                                    &channels, &cache,
                                ),
                            },
                            Err(e) => Action::LoadFailed {
                                scope: LoadScope::ChannelList(guild_id.clone()),
                                message: format!("failed to decode channel list: {e}"),
                            },
                        },
                        Err(e) => Action::LoadFailed {
                            scope: LoadScope::ChannelList(guild_id),
                            message: format!("failed to load channels: {e}"),
                        },
                    }
                };
                let _ = tx.send(action).await;
            });
        }
        Effect::LoadHistory { channel_id } => {
            let session = ds.session;
            let http = ds.client.http().clone();
            let cache = ds.client.cache().clone();
            let tx = tx.clone();
            let _ = tx.try_send(Action::LoadStarted(LoadScope::History(channel_id.clone())));
            tokio::spawn(async move {
                let action = fetch_history_action(&http, &cache, channel_id, session).await;
                let _ = tx.send(action).await;
            });
        }

        Effect::SendMessage {
            channel_id,
            content,
        } => {
            let http = ds.client.http().clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");
                let body = serde_json::json!({ "content": content });
                if let Err(e) = http.post(&url, body).await {
                    let _ = tx.send(Action::Error(format!("failed to send: {e}"))).await;
                }
            });
        }
        Effect::SummarizeChannel {
            channel_name,
            messages,
            user_name,
            ..
        } => {
            execute_summarize(tx, &channel_name, messages, &user_name, ai_backend);
        }
        Effect::GenerateDigest { job, request } => {
            execute_digest(
                tx,
                job,
                request,
                HistorySource::Discord(ds.client.clone()),
                ai_backend,
            );
        }
        Effect::FetchAvatar { url } => execute_fetch_avatar(tx, &url),
    }
}

#[cfg(feature = "experimental-discord")]
fn connect_discord_with_token(token: &str, tx: &mpsc::Sender<Action>) -> Result<DiscordState> {
    auth::log_tos_warning();
    let (client, handle) = transport::discord::connect(token.to_string(), tx.clone())?;
    Ok(DiscordState {
        session: std::time::Instant::now(),
        client,
        handle,
    })
}

#[cfg(feature = "experimental-discord")]
fn history_error(error: &diself::Error) -> String {
    if let diself::Error::Api { status: 401, .. } = error {
        return "Discord rejected the message session (HTTP 401). Sign in again.".into();
    }
    if let diself::Error::Api { status, .. } = error {
        return format!("Discord could not load this channel (HTTP {status}). Press r to retry.");
    }
    "Could not load channel history. Press r to retry.".into()
}

#[cfg(feature = "experimental-discord")]
async fn fetch_history_action(
    http: &diself::HttpClient,
    cache: &diself::Cache,
    channel_id: String,
    session: std::time::Instant,
) -> Action {
    let scope = LoadScope::History(channel_id.clone());
    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=50");
    let value = match tokio::time::timeout(std::time::Duration::from_secs(20), http.get(&url)).await
    {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let message = history_error(&error);
            if matches!(&error, diself::Error::Api { status: 401, .. }) {
                let account = tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    http.get("https://discord.com/api/v10/users/@me"),
                )
                .await;
                if matches!(account, Ok(Err(diself::Error::Api { status: 401, .. }))) {
                    return Action::MessageSessionRejected {
                        session,
                        channel_id,
                        message,
                    };
                }
            }
            return Action::LoadFailed { scope, message };
        }
        Err(_) => {
            return Action::LoadFailed {
                scope,
                message: "Channel history timed out. Press r to retry.".into(),
            };
        }
    };
    let raw_count = value.as_array().map_or(0, Vec::len);
    let payload_summary = history_payload_summary(&value);
    let mut rows = match transport::discord::history_rows_from_value(&channel_id, value) {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(raw_count, error = %err, payload = %payload_summary, "failed to decode discord history payload");
            return Action::LoadFailed {
                scope,
                message: "Could not decode channel history. Press r to retry.".into(),
            };
        }
    };
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        transport::discord::resolve_rows_user_mentions(http, cache, &mut rows),
    )
    .await;
    tracing::debug!(
        raw_count,
        decoded_count = rows.len(),
        "loaded discord history"
    );
    Action::HistoryLoaded {
        channel_id,
        messages: rows,
        has_more: raw_count >= 50,
    }
}

#[cfg(feature = "experimental-discord")]
fn history_payload_summary(value: &serde_json::Value) -> String {
    let Some(entries) = value.as_array() else {
        return match value {
            serde_json::Value::Object(map) => {
                let keys = map.keys().take(8).cloned().collect::<Vec<_>>().join(",");
                format!("non-array object keys=[{keys}]")
            }
            other => format!("non-array payload type={}", json_type_name(other)),
        };
    };

    let preview = entries
        .iter()
        .take(5)
        .map(history_entry_summary)
        .collect::<Vec<_>>()
        .join("; ");
    format!("entries={} preview=[{}]", entries.len(), preview)
}

#[cfg(feature = "experimental-discord")]
fn history_entry_summary(entry: &serde_json::Value) -> String {
    let id = entry
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let kind = entry
        .get("type")
        .and_then(serde_json::Value::as_u64)
        .map_or_else(|| "?".to_string(), |value| value.to_string());
    let flags = entry
        .get("flags")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let content_len = entry
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map_or(0, str::len);
    let attachments = entry
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let author_id = entry
        .get("author")
        .and_then(|author| author.get("id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");

    format!(
        "id={id} type={kind} author={author_id} content_len={content_len} attachments={attachments} flags={flags}"
    )
}

#[cfg(feature = "experimental-discord")]
fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// --- Shared: AI summarization ---

fn execute_summarize(
    tx: &mpsc::Sender<Action>,
    channel_name: &str,
    messages: Vec<crate::model::MessageRow>,
    user_name: &str,
    backend: Option<&SummarizerBackend>,
) {
    let Some(backend) = backend.cloned() else {
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx
                .send(Action::SummaryFailed(
                    crate::config::NO_AI_BACKEND_HELP.into(),
                ))
                .await;
        });
        return;
    };

    let tx = tx.clone();
    let ch_name = channel_name.to_string();
    let uname = user_name.to_string();
    tokio::spawn(async move {
        let action = match backend.summarize(&ch_name, &messages, &uname).await {
            Ok(digest) => Action::SummaryReady(digest),
            Err(e) => Action::SummaryFailed(format!("Summary failed: {e}")),
        };
        let _ = tx.send(action).await;
    });
}

fn execute_digest(
    tx: &mpsc::Sender<Action>,
    job: std::time::Instant,
    request: PreparedDigest,
    source: HistorySource,
    backend: Option<&SummarizerBackend>,
) {
    let tx = tx.clone();
    let backend = backend.cloned();
    tokio::spawn(async move {
        let report = request
            .generate(source, backend.as_ref(), |_, _, _| {})
            .await;
        let _ = tx.send(Action::DigestReady { job, report }).await;
    });
}

fn execute_fetch_avatar(tx: &mpsc::Sender<Action>, url: &str) {
    let tx = tx.clone();
    let url = url.to_string();
    tokio::spawn(async move {
        let action = match reqwest::get(&url).await {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => Action::AvatarLoaded {
                    url,
                    bytes: bytes.to_vec(),
                },
                Err(_) => Action::AvatarFailed(url),
            },
            _ => Action::AvatarFailed(url),
        };
        let _ = tx.send(action).await;
    });
}

fn handle_insert_input(evt: &Event, app: &mut App) {
    let Event::Key(key) = evt else { return };
    if key.kind != KeyEventKind::Press {
        return;
    }
    match key.code {
        KeyCode::Char(c) => app.input_text.push(c),
        KeyCode::Backspace => {
            app.input_text.pop();
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.input_text.push('\n');
        }
        _ => {}
    }
}

#[cfg(feature = "experimental-discord")]
fn map_sign_in_event(evt: &Event, app: &mut App) -> Option<Action> {
    let key = match evt {
        Event::Resize(width, height) => {
            return Some(Action::Resize {
                width: *width,
                height: *height,
            });
        }
        Event::Key(key) if key.kind == KeyEventKind::Press => key,
        _ => return None,
    };
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if control && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }

    if let Some(phase) = app.browser_phase() {
        if key.code == KeyCode::Char('q') {
            return Some(Action::Quit);
        }
        if matches!(phase, BrowserPhase::Restore | BrowserPhase::SigningOut(_)) {
            return None;
        }
        if *phase == BrowserPhase::Waiting {
            return (key.code == KeyCode::Esc).then_some(Action::CancelBrowser);
        }
        if *phase == BrowserPhase::AccountReady {
            return match key.code {
                KeyCode::Enter | KeyCode::Char('b') => Some(Action::BrowserSignIn),
                KeyCode::Esc => Some(Action::CancelDiscordToken),
                _ => None,
            };
        }
        if *phase == BrowserPhase::Ready && key.code == KeyCode::Enter {
            #[cfg(target_os = "macos")]
            return Some(Action::WebSignIn);
            #[cfg(not(target_os = "macos"))]
            return Some(Action::BrowserSignIn);
        }
        if matches!(phase, BrowserPhase::SignedIn(_)) {
            if matches!(key.code, KeyCode::Char('j') | KeyCode::Down) {
                return Some(Action::MoveDown);
            }
            if matches!(key.code, KeyCode::Char('k') | KeyCode::Up) {
                return Some(Action::MoveUp);
            }
            if key.code == KeyCode::Enter {
                return Some(Action::OpenSelected);
            }
            if key.code == KeyCode::Char('s') {
                return Some(Action::BrowserSignOut);
            }
        }
    }
    #[cfg(target_os = "macos")]
    if app.web_sign_in_open() {
        return (key.code == KeyCode::Esc)
            .then(|| Action::WebSignInFailed("Sign-in cancelled.".into()));
    }

    if app.sign_in_takes_text() {
        return match key.code {
            KeyCode::Enter => Some(Action::SubmitDiscordToken),
            KeyCode::Esc => Some(Action::LeaveTokenEntry),
            KeyCode::Backspace => {
                app.pop_token_char();
                None
            }
            KeyCode::Char(ch) if !control => {
                app.push_token_char(ch);
                None
            }
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char('b') => match app.browser_phase() {
            None | Some(BrowserPhase::Ready | BrowserPhase::AccountReady) => {
                Some(Action::BrowserSignIn)
            }
            Some(
                BrowserPhase::Restore
                | BrowserPhase::Waiting
                | BrowserPhase::SignedIn(_)
                | BrowserPhase::SigningOut(_),
            ) => None,
        },
        KeyCode::Char('t') => Some(Action::ShowTokenEntry),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Esc => Some(Action::CancelDiscordToken),
        _ => None,
    }
}

/// Handle an action while the sign-in prompt is open. Returns whether the
/// action was consumed.
#[cfg(feature = "experimental-discord")]
fn handle_sign_in_action(
    action: &Action,
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    ai_backend: Option<&SummarizerBackend>,
    discord: &mut DiscordRuntime,
) -> bool {
    if !app.has_sign_in() {
        return false;
    }

    match action {
        Action::MoveDown | Action::MoveUp | Action::OpenSelected => return false,
        Action::BrowserSignIn => discord.start_browser(app, oauth::Operation::Authorize),
        Action::BrowserSignOut => discord.start_browser(app, oauth::Operation::SignOut),
        Action::CancelBrowser => discord.cancel_browser(),
        Action::SubmitDiscordToken => {
            let Some(mut token) = app.take_token_input() else {
                return true;
            };
            if let Err(error) = connect_with_new_token(&token, app, tx, discord) {
                app.set_sign_in_error(error);
            }
            token.zeroize();
        }
        Action::ShowTokenEntry => app.show_token_entry(),
        // Back to the first screen, with the saved account if there is one.
        Action::LeaveTokenEntry => discord.start_browser(app, oauth::Operation::Restore),
        #[cfg(target_os = "macos")]
        Action::WebSignIn => discord.start_web_sign_in(app, tx),
        #[cfg(target_os = "macos")]
        Action::WebSignInProgress(stage) => app.set_web_stage(*stage),
        #[cfg(target_os = "macos")]
        Action::WebSignInSaved => {
            app.set_web_stage(crate::app::WebStage::Connecting);
            if let Err(error) = connect_with_saved_session(app, tx, discord) {
                discord.stop_web_sign_in();
                app.set_browser_phase(BrowserPhase::Ready);
                app.set_sign_in_error(error);
            }
        }
        #[cfg(target_os = "macos")]
        Action::WebSignInFailed(reason) => {
            discord.stop_web_sign_in();
            app.set_browser_phase(BrowserPhase::Ready);
            app.set_sign_in_error(reason.clone());
        }
        Action::CancelDiscordToken => {
            app.dismiss_sign_in();
            if !discord.transport_started {
                start_mock_transport(tx, ai_backend);
                discord.transport_started = true;
            }
            app.connection_state = ConnectionState::MockTransport;
        }
        Action::Quit | Action::Tick | Action::Resize { .. } => {
            let _ = app.update(action.clone());
        }
        _ if action.origin() == crate::action::Origin::Delivered => return false,
        _ => {}
    }
    true
}

/// Connect with the session the sign-in window just saved.
///
/// The Keychain copy is used exactly as saved. Re-storing it would drop the
/// client properties saved beside it, and Discord revokes a session whose
/// Gateway connection identifies as a different client.
#[cfg(all(target_os = "macos", feature = "experimental-discord"))]
fn connect_with_saved_session(
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    discord: &mut DiscordRuntime,
) -> Result<(), String> {
    let mut token = auth::get_saved_token().map_err(|err| err.to_string())?;
    app.start_load_scope(LoadScope::StartupConnect);
    let connected = connect_discord_with_token(&token, tx);
    token.zeroize();
    match connected {
        Ok(state) => {
            discord.stop_web_sign_in();
            app.dismiss_sign_in();
            let _ = app.update(Action::TransportConnecting);
            discord.state = Some(state);
            discord.transport_started = true;
            Ok(())
        }
        Err(err) => {
            app.finish_load_scope(&LoadScope::StartupConnect);
            Err(format!("Failed to connect: {err}"))
        }
    }
}

/// Save a token the user just supplied and connect with it. On success the
/// sign-in prompt closes; on failure the reason comes back for the prompt.
#[cfg(feature = "experimental-discord")]
fn connect_with_new_token(
    token: &str,
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    discord: &mut DiscordRuntime,
) -> Result<(), String> {
    auth::store_token(token).map_err(|err| err.to_string())?;

    app.start_load_scope(LoadScope::StartupConnect);
    match connect_discord_with_token(token, tx) {
        Ok(state) => {
            app.dismiss_sign_in();
            let _ = app.update(Action::TransportConnecting);
            discord.state = Some(state);
            discord.transport_started = true;
            Ok(())
        }
        Err(err) => {
            app.finish_load_scope(&LoadScope::StartupConnect);
            Err(format!("Failed to connect: {err}"))
        }
    }
}

// The async tests here abort or drop the account restore they start before
// it is polled, so it never reaches the Keychain.
#[cfg(all(test, feature = "experimental-discord"))]
mod sign_in_tests {
    use crossterm::event::KeyEvent;

    use crate::app::SignInMode;
    use crate::*;

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn mode(app: &App) -> Option<&SignInMode> {
        app.sign_in().map(|sign_in| &sign_in.mode)
    }

    #[tokio::test]
    async fn restored_browser_account_is_visible_without_starting_a_message_transport() {
        let mut app = App::new();
        app.set_browser_phase(BrowserPhase::Restore);
        let mut runtime = DiscordRuntime {
            browser: Some(tokio::spawn(async {
                Ok(oauth::Outcome::Account("pilot".into(), vec![]))
            })),
            ..DiscordRuntime::default()
        };
        let result = runtime.browser_result().await;
        runtime.finish_browser(&mut app, result);
        assert_eq!(app.browser_account(), Some("pilot"));
        assert!(runtime.state.is_none());
        assert!(matches!(
            map_runtime_event(&press(KeyCode::Char('s')), &mut app),
            Some(Action::BrowserSignOut)
        ));
        app.set_browser_phase(BrowserPhase::Waiting);
        assert!(matches!(
            map_runtime_event(&press(KeyCode::Esc), &mut app),
            Some(Action::CancelBrowser)
        ));
        runtime.finish_browser(&mut app, Ok(oauth::Outcome::Ready));
        assert!(is_ready_action(
            map_runtime_event(&press(KeyCode::Enter), &mut app).as_ref()
        ));
    }

    /// Enter on the first prompt starts the sign-in that reaches messages on
    /// macOS, and the browser account flow everywhere else.
    fn is_ready_action(action: Option<&Action>) -> bool {
        #[cfg(target_os = "macos")]
        return matches!(action, Some(Action::WebSignIn));
        #[cfg(not(target_os = "macos"))]
        return matches!(action, Some(Action::BrowserSignIn));
    }

    #[tokio::test]
    async fn account_launch_without_a_grant_keeps_enter_on_oauth() {
        let mut app = App::new();
        let (tx, _) = mpsc::channel(1);
        let mut runtime = DiscordRuntime::default();
        runtime.start(&mut app, &tx, true);
        // Restore hasn't been polled. Use its no-grant outcome without touching Keychain.
        if let Some(task) = runtime.browser.take() {
            task.abort();
            let _ = task.await;
        }
        runtime.finish_browser(&mut app, Ok(oauth::Outcome::Ready));
        assert_eq!(app.browser_phase(), Some(&BrowserPhase::AccountReady));
        assert!(matches!(
            map_runtime_event(&press(KeyCode::Enter), &mut app),
            Some(Action::BrowserSignIn)
        ));
    }

    #[test]
    fn pending_work_completes_behind_sign_in() {
        let mut app = App::new();
        let mut runtime = DiscordRuntime::default();
        let (tx, _) = mpsc::channel(8);
        let channel = "c1".to_owned();
        app.selected_channel_id = Some(channel.clone());
        app.start_load_scope(LoadScope::History(channel.clone()));
        app.summary_state.in_flight = true;
        app.set_browser_phase(BrowserPhase::AccountReady);
        for action in [
            Action::HistoryLoaded {
                channel_id: channel,
                messages: vec![],
                has_more: false,
            },
            Action::SummaryFailed("offline".into()),
        ] {
            process_action(action, &mut app, &tx, None, &mut runtime);
        }
        assert!(app.active_load_label().is_none());
        assert!(!app.summary_state.in_flight);
        assert_eq!(app.browser_phase(), Some(&BrowserPhase::AccountReady));
    }

    #[test]
    fn guild_permission_upgrade_offers_interactive_browser_consent() {
        let mut app = App::new();
        app.set_browser_phase(BrowserPhase::Restore);
        DiscordRuntime::default().finish_browser(&mut app, Ok(oauth::Outcome::NeedsConsent));
        assert_eq!(app.browser_phase(), Some(&BrowserPhase::Ready));
        assert!(
            app.sign_in()
                .and_then(|prompt| prompt.error.as_deref())
                .is_some_and(|error| error.contains("permission has changed"))
        );
        assert!(matches!(
            map_runtime_event(&press(KeyCode::Char('b')), &mut app),
            Some(Action::BrowserSignIn)
        ));
    }

    #[test]
    fn rejected_history_stays_visible_after_the_footer_error_expires() {
        let mut app = App::new();
        app.selected_channel_id = Some("channel".into());
        let error = diself::Error::Api {
            status: 401,
            body: "sensitive reply".into(),
        };
        let message = history_error(&error);
        assert!(!message.contains("sensitive reply"));
        app.update(Action::LoadFailed {
            scope: LoadScope::History("channel".into()),
            message,
        });
        assert!(
            app.selected_history_status()
                .is_some_and(|text| text.contains("HTTP 401"))
        );
        app.update(Action::LoadStarted(LoadScope::History("channel".into())));
        assert_eq!(app.selected_history_status(), Some("Loading messages…"));
    }

    #[tokio::test]
    async fn a_disconnected_live_session_does_not_load_fixture_history() {
        let (tx, mut rx) = mpsc::channel(4);
        dispatch_effects(
            vec![Effect::LoadHistory {
                channel_id: "channel".into(),
            }],
            &tx,
            None,
            None,
            false,
        );
        assert!(rx.try_recv().is_err());
        dispatch_effects(
            vec![Effect::LoadHistory {
                channel_id: "channel".into(),
            }],
            &tx,
            None,
            None,
            true,
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(Action::LoadStarted(LoadScope::History(_)))
        ));
    }

    #[tokio::test]
    async fn a_disconnected_live_session_still_answers_summaries_and_sends() {
        let (tx, mut rx) = mpsc::channel(4);
        dispatch_effects(
            vec![Effect::SummarizeChannel {
                channel_id: "channel".into(),
                channel_name: "general".into(),
                messages: mock::messages("c1"),
                user_name: mock::USER_NAME.into(),
            }],
            &tx,
            None,
            None,
            false,
        );
        let answer = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await;
        let Ok(Some(Action::SummaryFailed(reason))) = answer else {
            unreachable!("a summary request must end, got {answer:?}");
        };
        assert_eq!(reason, crate::config::NO_AI_BACKEND_HELP);

        dispatch_effects(
            vec![Effect::SendMessage {
                channel_id: "channel".into(),
                content: "hello".into(),
            }],
            &tx,
            None,
            None,
            false,
        );
        let Ok(Action::Error(reason)) = rx.try_recv() else {
            unreachable!("an unsent message must be reported");
        };
        assert!(reason.contains("not sent"), "{reason}");
    }

    #[test]
    fn the_first_prompt_takes_b_for_the_account_only_sign_in() {
        let mut app = App::new();
        app.set_browser_phase(BrowserPhase::Ready);
        assert!(matches!(
            map_sign_in_event(&press(KeyCode::Char('b')), &mut app),
            Some(Action::BrowserSignIn)
        ));

        app.set_browser_phase(BrowserPhase::Restore);
        assert!(map_sign_in_event(&press(KeyCode::Char('b')), &mut app).is_none());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_sign_in_window_opens_once_and_esc_closes_it() {
        let mut app = App::new();
        app.set_browser_phase(BrowserPhase::Ready);
        let (tx, _rx) = mpsc::channel::<Action>(8);
        let mut runtime = DiscordRuntime::default();

        runtime.start_web_sign_in(&mut app, &tx);
        assert!(app.web_sign_in_open());
        let first = runtime
            .web_sign_in
            .as_ref()
            .map(tokio::task::JoinHandle::id);

        // A second request must not race the first for the same Keychain entry.
        runtime.start_web_sign_in(&mut app, &tx);
        assert_eq!(
            runtime
                .web_sign_in
                .as_ref()
                .map(tokio::task::JoinHandle::id),
            first
        );

        let Some(Action::WebSignInFailed(reason)) =
            map_sign_in_event(&press(KeyCode::Esc), &mut app)
        else {
            unreachable!("esc cancels the sign-in window");
        };
        assert_eq!(reason, "Sign-in cancelled.");
        runtime.stop_web_sign_in();
        assert!(runtime.web_sign_in.is_none());
    }

    #[test]
    fn first_prompt_keys_choose_what_happens_next() {
        let mut app = App::new();
        app.show_sign_in();

        let Some(Action::ShowTokenEntry) = map_sign_in_event(&press(KeyCode::Char('t')), &mut app)
        else {
            unreachable!("t opens the token field");
        };
        let Some(Action::CancelDiscordToken) = map_sign_in_event(&press(KeyCode::Esc), &mut app)
        else {
            unreachable!("esc leaves for sample data");
        };
        let Some(Action::Quit) = map_sign_in_event(&press(KeyCode::Char('q')), &mut app) else {
            unreachable!("q quits");
        };
        for unbound in ['r', 'x'] {
            assert!(map_sign_in_event(&press(KeyCode::Char(unbound)), &mut app).is_none());
        }
        assert_eq!(mode(&app), Some(&SignInMode::Browser(BrowserPhase::Ready)));
    }

    #[test]
    fn token_field_takes_letters_that_are_commands_on_the_first_prompt() {
        let mut app = App::new();
        app.show_sign_in();
        app.show_token_entry();

        for ch in ['t', 'b', 'q'] {
            assert!(map_sign_in_event(&press(KeyCode::Char(ch)), &mut app).is_none());
        }
        assert!(map_sign_in_event(&press(KeyCode::Backspace), &mut app).is_none());
        assert_eq!(mode(&app), Some(&SignInMode::Token { input: "tb".into() }));

        let Some(Action::SubmitDiscordToken) = map_sign_in_event(&press(KeyCode::Enter), &mut app)
        else {
            unreachable!("enter submits the token");
        };
        let Some(Action::LeaveTokenEntry) = map_sign_in_event(&press(KeyCode::Esc), &mut app)
        else {
            unreachable!("esc goes back to the first screen");
        };
        let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        let Some(Action::Quit) = map_sign_in_event(&ctrl_c, &mut app) else {
            unreachable!("ctrl+c quits from the token field");
        };
    }

    #[tokio::test]
    async fn leaving_the_token_field_returns_to_the_first_screen() {
        let mut app = App::new();
        let (tx, _rx) = mpsc::channel(8);
        let mut discord = DiscordRuntime::default();
        app.show_sign_in();

        assert!(handle_sign_in_action(
            &Action::ShowTokenEntry,
            &mut app,
            &tx,
            None,
            &mut discord
        ));
        assert!(app.sign_in_takes_text());

        app.set_sign_in_error("Failed to connect".into());
        assert!(handle_sign_in_action(
            &Action::LeaveTokenEntry,
            &mut app,
            &tx,
            None,
            &mut discord
        ));
        assert!(!app.sign_in_takes_text());
        assert_eq!(app.browser_phase(), Some(&BrowserPhase::Restore));
        assert_eq!(
            app.sign_in().and_then(|sign_in| sign_in.error.as_deref()),
            None
        );

        // The restore was never polled; finish it as "no saved account".
        if let Some(task) = discord.browser.take() {
            task.abort();
            let _ = task.await;
        }
        discord.finish_browser(&mut app, Ok(oauth::Outcome::Ready));
        assert_eq!(app.browser_phase(), Some(&BrowserPhase::Ready));
    }

    #[tokio::test]
    async fn input_actions_do_not_leak_past_the_prompt() {
        let mut app = App::new();
        let (tx, _rx) = mpsc::channel(8);
        let mut discord = DiscordRuntime::default();
        app.show_sign_in();

        let focus = app.focus;
        assert!(handle_sign_in_action(
            &Action::FocusNext,
            &mut app,
            &tx,
            None,
            &mut discord
        ));
        assert_eq!(app.focus, focus);
    }

    #[tokio::test]
    async fn open_received_during_sign_in_survives_until_guilds_arrive() {
        let mut app = App::new();
        let (tx, mut rx) = mpsc::channel(8);
        let mut discord = DiscordRuntime::default();
        app.show_sign_in();

        process_action(
            Action::OpenChannel("c20".into()),
            &mut app,
            &tx,
            None,
            &mut discord,
        );
        for _ in 0..200 {
            process_action(Action::Tick, &mut app, &tx, None, &mut discord);
        }
        assert!(
            rx.try_recv().is_err(),
            "must defer all fetches until sign-in finishes"
        );

        app.dismiss_sign_in();
        app.update(Action::TransportSessionExpired);
        assert_eq!(app.connection_state, ConnectionState::SessionExpired);
        for _ in 0..200 {
            assert!(app.update(Action::Tick).is_empty());
        }
        assert_eq!(app.browser_phase(), Some(&BrowserPhase::Ready));
        assert!(
            app.sign_in()
                .and_then(|prompt| prompt.error.as_ref())
                .is_some()
        );
        app.dismiss_sign_in();
        app.update(Action::TransportDisconnected("Network unavailable".into()));
        assert_eq!(app.connection_state, ConnectionState::Disconnected);
        for _ in 0..200 {
            assert!(app.update(Action::Tick).is_empty());
        }
        app.update(Action::TransportConnecting);
        for _ in 0..200 {
            app.update(Action::Tick);
        }
        app.update(Action::TransportConnected {
            username: mock::USER_NAME.into(),
        });
        for _ in 0..200 {
            app.update(Action::Tick);
        }
        app.update(Action::ReadyData {
            guilds: mock::guilds(),
            guild_channels: [("g3".into(), mock::channels("g3"))].into(),
            guild_mute_settings: std::collections::HashMap::new(),
        });
        let effects = app.update(Action::Tick);

        assert_eq!(app.selected_channel_id.as_deref(), Some("c20"));
        assert!(effects.iter().any(|effect| matches!(
            effect, Effect::LoadHistory { channel_id } if channel_id == "c20"
        )));
    }

    #[tokio::test]
    async fn leaving_the_prompt_starts_sample_data_once() {
        let mut app = App::new();
        let (tx, mut rx) = mpsc::channel(8);
        let mut discord = DiscordRuntime::default();
        app.show_sign_in();

        assert!(handle_sign_in_action(
            &Action::CancelDiscordToken,
            &mut app,
            &tx,
            None,
            &mut discord
        ));
        assert!(!app.has_sign_in());
        assert!(discord.transport_started);
        assert_eq!(app.connection_state, ConnectionState::MockTransport);
        let Ok(Action::LoadStarted(LoadScope::GuildBootstrap)) = rx.try_recv() else {
            unreachable!("sample data starts loading");
        };

        // With the prompt closed, sign-in actions are no longer consumed here.
        assert!(!handle_sign_in_action(
            &Action::ShowTokenEntry,
            &mut app,
            &tx,
            None,
            &mut discord
        ));
        assert!(!app.has_sign_in());
    }
}

#[cfg(test)]
mod digest_view_tests {
    use crossterm::event::KeyEvent;
    use ratatui::{Terminal, backend::TestBackend};

    use crate::*;

    #[tokio::test]
    #[expect(
        clippy::unwrap_used,
        reason = "test setup and key mappings must succeed"
    )]
    async fn digest_rail_opens_the_sample_report_and_its_selected_channel() {
        let mut app = App::new();
        let guilds = mock::guilds();
        for guild in &guilds {
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild.id.clone()),
                channels: mock::channels(&guild.id),
            });
        }
        app.update(Action::GuildsLoaded(guilds));
        let (tx, mut rx) = mpsc::channel(8);
        #[cfg(feature = "experimental-discord")]
        let mut discord = DiscordRuntime::default();
        for key in [KeyCode::Char('1'), KeyCode::Char('g'), KeyCode::Enter] {
            let event = Event::Key(KeyEvent::new(key, KeyModifiers::NONE));
            let action = map_runtime_event(&event, &mut app).unwrap();
            process_action(
                action,
                &mut app,
                &tx,
                None,
                #[cfg(feature = "experimental-discord")]
                &mut discord,
            );
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .unwrap();
        let Some(action @ Action::DigestReady { .. }) = result else {
            unreachable!("the digest worker should return a report");
        };
        app.update(action);

        let mut terminal = Terminal::new(TestBackend::new(116, 32)).unwrap();
        terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
        let screen = terminal.backend().to_string();
        for label in [
            "◆ Digest",
            "NEEDS YOU",
            "TOP CHANNELS",
            "QUIET",
            "Nix/NixOS",
        ] {
            assert!(screen.contains(label), "missing {label} in {screen}");
        }
        app.update(Action::OpenSelected);
        assert_eq!(app.selected_channel_id.as_deref(), Some("c1"));
        assert!(!app.digest.visible);
    }
}
