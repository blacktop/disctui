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

use crate::action::Action;
use crate::ai::Summarizer;
use crate::ai::SummarizerBackend;
use crate::ai::claude::ClaudeSummarizer;
use crate::ai::local::LocalSummarizer;
use crate::app::{App, ConnectionState, InputMode};
use crate::config::AppConfig;
use crate::effect::Effect;
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
    let discord = try_connect_discord(&action_tx);
    #[cfg(feature = "experimental-discord")]
    let use_discord = discord.is_some();
    #[cfg(not(feature = "experimental-discord"))]
    let use_discord = false;

    if use_discord {
        app.connection_state = ConnectionState::Connecting;
    } else {
        let init = App::init_effects();
        for effect in init {
            execute_effect_mock(effect, &action_tx, ai_backend.as_ref());
        }
    }

    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        let action = tokio::select! {
            Some(Ok(evt)) = term_events.next() => {
                let mapped = event::map_terminal_event(&evt, app.input_mode, app.focus);
                if mapped.is_none() && app.input_mode == InputMode::Insert {
                    handle_insert_input(&evt, &mut app);
                }
                mapped
            }
            Some(action) = action_rx.recv() => Some(action),
            _ = tick.tick() => Some(Action::Tick),
        };

        if let Some(action) = action {
            let effects = app.update(action);
            dispatch_effects(
                effects,
                &action_tx,
                ai_backend.as_ref(),
                #[cfg(feature = "experimental-discord")]
                discord.as_ref(),
            );
        }

        while let Ok(action) = action_rx.try_recv() {
            let effects = app.update(action);
            dispatch_effects(
                effects,
                &action_tx,
                ai_backend.as_ref(),
                #[cfg(feature = "experimental-discord")]
                discord.as_ref(),
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
            let guilds = mock::guilds();
            tokio::spawn(async move {
                let _ = tx.send(Action::GuildsLoaded(guilds)).await;
            });
        }
        Effect::LoadChannels { guild_id } => {
            let tx = tx.clone();
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
            let tx = tx.clone();
            tokio::spawn(async move {
                let url = format!("https://discord.com/api/v10/guilds/{guild_id}/channels");
                let action = match http.get(&url).await {
                    Ok(value) => {
                        let channels: Vec<diself::Channel> =
                            serde_json::from_value(value).unwrap_or_default();
                        let summaries: Vec<_> = channels
                            .iter()
                            .filter_map(transport::discord::channel_to_summary)
                            .collect();
                        Action::ChannelsLoaded {
                            guild_id: Some(guild_id),
                            channels: summaries,
                        }
                    }
                    Err(e) => Action::Error(format!("failed to load channels: {e}")),
                };
                let _ = tx.send(action).await;
            });
        }
        Effect::LoadHistory { channel_id } => {
            let http = ds.client.http().clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let url =
                    format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=50");
                let action = match http.get(&url).await {
                    Ok(value) => {
                        let msgs: Vec<diself::Message> =
                            serde_json::from_value(value).unwrap_or_default();
                        let has_more = msgs.len() >= 50;
                        let rows = transport::discord::messages_to_rows(&msgs);
                        Action::HistoryLoaded {
                            channel_id,
                            messages: rows,
                            has_more,
                        }
                    }
                    Err(e) => Action::Error(format!("failed to load history: {e}")),
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
fn try_connect_discord(tx: &mpsc::Sender<Action>) -> Option<DiscordState> {
    let token = match auth::get_token() {
        Ok(t) => t,
        Err(e) => {
            tracing::info!("no discord token, using mock transport: {e}");
            return None;
        }
    };
    auth::log_tos_warning();
    match transport::discord::connect(token, tx.clone()) {
        Ok((client, handle)) => Some(DiscordState { client, handle }),
        Err(e) => {
            tracing::error!("failed to connect to discord: {e}");
            None
        }
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
