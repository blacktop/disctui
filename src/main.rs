mod action;
mod ai;
mod app;
#[cfg(feature = "experimental-discord")]
mod auth;
mod config;
mod effect;
mod event;
mod logging;
mod model;
mod store;
mod transport;
mod tui;
mod ui;

use clap::Parser;
use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::time::interval;
#[cfg(feature = "experimental-discord")]
use zeroize::Zeroize;

use crate::action::Action;
use crate::ai::Summarizer;
use crate::ai::SummarizerBackend;
use crate::ai::claude::ClaudeSummarizer;
use crate::ai::local::LocalSummarizer;
#[cfg(feature = "experimental-discord")]
use crate::app::ConnectionState;
use crate::app::{App, InputMode};
use crate::config::AppConfig;
use crate::effect::Effect;
#[cfg(not(feature = "experimental-discord"))]
use crate::model::LoadScope;
#[cfg(feature = "experimental-discord")]
use crate::model::{DIRECT_MESSAGES_GUILD_ID, LoadScope};
use crate::transport::mock;

const ACTION_CHANNEL_CAPACITY: usize = 512;

#[derive(Parser)]
#[command(name = "disctui")]
#[command(about = "A fast, minimal Discord TUI with AI summaries")]
struct Cli {
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    #[arg(short, long)]
    debug: bool,
}

#[cfg(feature = "experimental-discord")]
struct DiscordState {
    client: std::sync::Arc<diself::Client>,
    handle: tokio::task::JoinHandle<()>,
}

// Single-threaded: Rc<Store> is not Send, so we must not migrate futures across threads.
// A TUI has no need for multi-threaded work-stealing — all I/O is async via tokio.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();
    let config = AppConfig::load(cli.config.as_deref())?;
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
    let result = run(&mut terminal, &config, avatars, store).await;
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
) -> Result<()> {
    let mut app = App::new_with_avatars(avatars);
    app.store = Some(store);
    let (action_tx, mut action_rx) = mpsc::channel::<Action>(ACTION_CHANNEL_CAPACITY);
    let mut term_events = EventStream::new();
    let mut tick = interval(config.tick_rate());
    let ai_backend = config.summarizer_backend();
    tick.tick().await;

    #[cfg(feature = "experimental-discord")]
    let mut discord = None;
    #[cfg(feature = "experimental-discord")]
    let mut transport_started = false;
    #[cfg(feature = "experimental-discord")]
    match auth::get_token() {
        Ok(token) => match connect_discord_with_token(&token, &action_tx) {
            Ok(state) => {
                app.start_load_scope(LoadScope::StartupConnect);
                discord = Some(state);
                app.connection_state = ConnectionState::Connecting;
                transport_started = true;
            }
            Err(err) => {
                tracing::error!("failed to connect to discord: {err}");
                app.show_discord_token_prompt();
                app.set_discord_token_prompt_error(format!("Saved token failed to connect: {err}"));
            }
        },
        Err(err) => {
            tracing::info!("discord token unavailable at startup: {err}");
            app.show_discord_token_prompt();
        }
    }
    #[cfg(not(feature = "experimental-discord"))]
    {
        start_mock_transport(&action_tx, ai_backend.as_ref());
    }

    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        let action = tokio::select! {
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
                #[cfg(feature = "experimental-discord")]
                &mut transport_started,
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
                #[cfg(feature = "experimental-discord")]
                &mut transport_started,
            );
        }

        if app.should_quit {
            app.save_on_quit();
            break;
        }
    }

    #[cfg(feature = "experimental-discord")]
    if let Some(ds) = discord {
        ds.client.shutdown();
        let _ = ds.handle.await;
    }

    Ok(())
}

fn start_mock_transport(tx: &mpsc::Sender<Action>, ai_backend: Option<&SummarizerBackend>) {
    for effect in App::init_effects() {
        execute_effect_mock(effect, tx, ai_backend);
    }
}

fn map_runtime_event(evt: &Event, app: &mut App) -> Option<Action> {
    #[cfg(feature = "experimental-discord")]
    if app.has_discord_token_prompt() {
        return map_discord_token_prompt_event(evt, app);
    }

    let mapped = event::map_terminal_event(evt, app.input_mode, app.focus);
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
    #[cfg(feature = "experimental-discord")] discord: &mut Option<DiscordState>,
    #[cfg(feature = "experimental-discord")] transport_started: &mut bool,
) {
    #[cfg(feature = "experimental-discord")]
    if handle_discord_token_prompt_action(&action, app, tx, ai_backend, discord, transport_started)
    {
        return;
    }

    #[cfg(feature = "experimental-discord")]
    if handle_reconnect_action(&action, app, tx, discord, transport_started) {
        return;
    }

    #[cfg(feature = "experimental-discord")]
    let should_dispose_discord = matches!(action, Action::TransportDisconnected(_));
    let effects = app.update(action);
    #[cfg(feature = "experimental-discord")]
    if should_dispose_discord {
        dispose_discord_state(discord, false);
    }
    dispatch_effects(
        effects,
        tx,
        ai_backend,
        #[cfg(feature = "experimental-discord")]
        discord.as_ref(),
    );
}

#[cfg(feature = "experimental-discord")]
fn handle_reconnect_action(
    action: &Action,
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    discord: &mut Option<DiscordState>,
    transport_started: &mut bool,
) -> bool {
    if !matches!(action, Action::ReconnectTransport) {
        return false;
    }

    match app.connection_state {
        ConnectionState::Disconnected | ConnectionState::MockTransport => {
            reconnect_discord(app, tx, discord, transport_started);
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
fn reconnect_discord(
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    discord: &mut Option<DiscordState>,
    transport_started: &mut bool,
) {
    let preserve_mock_transport = app.connection_state == ConnectionState::MockTransport;
    dispose_discord_state(discord, true);

    let Ok(mut token) = auth::get_token() else {
        show_reconnect_prompt(
            app,
            transport_started,
            preserve_mock_transport,
            "Reconnect requires a saved Discord token",
        );
        return;
    };

    app.start_load_scope(LoadScope::StartupConnect);
    match connect_discord_with_token(&token, tx) {
        Ok(state) => {
            app.connection_state = ConnectionState::Reconnecting;
            *discord = Some(state);
            *transport_started = true;
        }
        Err(err) => {
            app.finish_load_scope(&LoadScope::StartupConnect);
            show_reconnect_prompt(
                app,
                transport_started,
                preserve_mock_transport,
                format!("Reconnect failed: {err}"),
            );
        }
    }
    token.zeroize();
}

#[cfg(feature = "experimental-discord")]
fn show_reconnect_prompt(
    app: &mut App,
    transport_started: &mut bool,
    preserve_mock_transport: bool,
    message: impl Into<String>,
) {
    if !preserve_mock_transport {
        *transport_started = false;
    }
    app.show_discord_token_prompt();
    app.set_discord_token_prompt_error(message.into());
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
) {
    for effect in effects {
        #[cfg(feature = "experimental-discord")]
        if let Some(ds) = discord {
            execute_effect_discord(effect, tx, ds, ai_backend);
        } else {
            execute_effect_mock(effect, tx, ai_backend);
        }
        #[cfg(not(feature = "experimental-discord"))]
        execute_effect_mock(effect, tx, ai_backend);
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
        Effect::FetchAvatar { url } => execute_fetch_avatar(tx, &url),
    }
}

#[cfg(feature = "experimental-discord")]
#[expect(
    clippy::too_many_lines,
    reason = "live Discord effect execution groups all REST-backed load paths in one dispatcher"
)]
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
                        Ok(channels) => {
                            let mut summaries =
                                transport::discord::channels_to_summaries(&channels);
                            transport::discord::apply_read_state_from_cache(&mut summaries, &cache);
                            Action::ChannelsLoaded {
                                guild_id: Some(guild_id),
                                channels: summaries,
                            }
                        }
                        Err(e) => Action::LoadFailed {
                            scope: LoadScope::ChannelList(guild_id),
                            message: format!("failed to load direct messages: {e}"),
                        },
                    }
                } else {
                    let url = format!("https://discord.com/api/v10/guilds/{guild_id}/channels");
                    match http.get(&url).await {
                        Ok(value) => match serde_json::from_value::<Vec<diself::Channel>>(value) {
                            Ok(channels) => {
                                let mut summaries =
                                    transport::discord::channels_to_summaries(&channels);
                                transport::discord::apply_read_state_from_cache(
                                    &mut summaries,
                                    &cache,
                                );
                                Action::ChannelsLoaded {
                                    guild_id: Some(guild_id),
                                    channels: summaries,
                                }
                            }
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
            let http = ds.client.http().clone();
            let tx = tx.clone();
            let _ = tx.try_send(Action::LoadStarted(LoadScope::History(channel_id.clone())));
            tokio::spawn(async move {
                let url =
                    format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=50");
                let action = match http.get(&url).await {
                    Ok(value) => {
                        let raw_count = value.as_array().map_or(0, Vec::len);
                        let payload_summary = history_payload_summary(&value);
                        match transport::discord::history_rows_from_value(&channel_id, value) {
                            Ok(rows) => {
                                let has_more = raw_count >= 50;
                                tracing::debug!(
                                    channel_id = %channel_id,
                                    raw_count,
                                    decoded_count = rows.len(),
                                    has_more,
                                    payload = %payload_summary,
                                    "loaded discord history"
                                );
                                Action::HistoryLoaded {
                                    channel_id,
                                    messages: rows,
                                    has_more,
                                }
                            }
                            Err(err) => {
                                tracing::warn!(
                                    channel_id = %channel_id,
                                    raw_count,
                                    error = %err,
                                    payload = %payload_summary,
                                    "failed to decode discord history payload"
                                );
                                Action::HistoryLoaded {
                                    channel_id,
                                    messages: Vec::new(),
                                    has_more: false,
                                }
                            }
                        }
                    }
                    Err(e) => Action::LoadFailed {
                        scope: LoadScope::History(channel_id),
                        message: format!("failed to load history: {e}"),
                    },
                };
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
        Effect::FetchAvatar { url } => execute_fetch_avatar(tx, &url),
    }
}

#[cfg(feature = "experimental-discord")]
fn connect_discord_with_token(token: &str, tx: &mpsc::Sender<Action>) -> Result<DiscordState> {
    auth::log_tos_warning();
    let (client, handle) = transport::discord::connect(token.to_string(), tx.clone())?;
    Ok(DiscordState { client, handle })
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
                    "No AI backend configured. Set ANTHROPIC_API_KEY or ai_backend = \"local\""
                        .into(),
                ))
                .await;
        });
        return;
    };

    let tx = tx.clone();
    let ch_name = channel_name.to_string();
    let uname = user_name.to_string();
    tokio::spawn(async move {
        let prepared = crate::ai::prepare_messages_for_summary(&messages);
        let action = match backend {
            SummarizerBackend::Claude { api_key } => {
                let summarizer = ClaudeSummarizer::new(api_key);
                match summarizer.summarize(&ch_name, &prepared, &uname).await {
                    Ok(digest) => Action::SummaryReady(digest),
                    Err(e) => Action::SummaryFailed(format!("Summary failed: {e}")),
                }
            }
            SummarizerBackend::Local { base_url, model } => {
                let summarizer = LocalSummarizer::new(Some(base_url), Some(model));
                match summarizer.summarize(&ch_name, &prepared, &uname).await {
                    Ok(digest) => Action::SummaryReady(digest),
                    Err(e) => Action::SummaryFailed(format!("Summary failed: {e}")),
                }
            }
        };
        let _ = tx.send(action).await;
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
fn map_discord_token_prompt_event(evt: &Event, app: &mut App) -> Option<Action> {
    match evt {
        Event::Resize(width, height) => Some(Action::Resize {
            width: *width,
            height: *height,
        }),
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Some(Action::Quit);
            }

            match key.code {
                KeyCode::Enter => Some(Action::SubmitDiscordToken),
                KeyCode::Esc => Some(Action::CancelDiscordToken),
                KeyCode::Backspace => {
                    app.pop_discord_token_prompt_char();
                    None
                }
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.push_discord_token_prompt_char(ch);
                    None
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(feature = "experimental-discord")]
fn handle_discord_token_prompt_action(
    action: &Action,
    app: &mut App,
    tx: &mpsc::Sender<Action>,
    ai_backend: Option<&SummarizerBackend>,
    discord: &mut Option<DiscordState>,
    transport_started: &mut bool,
) -> bool {
    if !app.has_discord_token_prompt() {
        return false;
    }

    match action {
        Action::SubmitDiscordToken => {
            let Some(mut token) = app.take_discord_token_prompt_input() else {
                return true;
            };

            app.start_load_scope(LoadScope::StartupConnect);
            if let Err(err) = auth::store_token(&token) {
                app.finish_load_scope(&LoadScope::StartupConnect);
                app.set_discord_token_prompt_error(err.to_string());
                token.zeroize();
                return true;
            }

            match connect_discord_with_token(&token, tx) {
                Ok(state) => {
                    app.dismiss_discord_token_prompt();
                    let _ = app.update(Action::TransportConnecting);
                    *discord = Some(state);
                    *transport_started = true;
                }
                Err(err) => {
                    app.finish_load_scope(&LoadScope::StartupConnect);
                    app.set_discord_token_prompt_error(format!("Failed to connect: {err}"));
                }
            }

            token.zeroize();
            true
        }
        Action::CancelDiscordToken => {
            app.dismiss_discord_token_prompt();
            if !*transport_started {
                start_mock_transport(tx, ai_backend);
                *transport_started = true;
            }
            app.connection_state = ConnectionState::MockTransport;
            true
        }
        Action::Quit => {
            let _ = app.update(Action::Quit);
            true
        }
        Action::Tick | Action::Resize { .. } => {
            let _ = app.update(action.clone());
            true
        }
        _ => true,
    }
}
