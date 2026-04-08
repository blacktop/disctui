use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use ratatui::widgets::ListState;

use crate::action::Action;
use crate::effect::Effect;
use crate::model::{
    ChannelDigest, ChannelKind, ChannelSummary, DIRECT_MESSAGES_GUILD_ID,
    DIRECT_MESSAGES_GUILD_NAME, GuildMuteSettings, GuildSummary, LoadScope, MessageRow,
    sort_channels_for_sidebar,
};
use crate::store::{self, Store};
use crate::ui::media::AvatarStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Guilds,
    Channels,
    Messages,
    Input,
}

impl FocusPane {
    pub fn label(self) -> &'static str {
        match self {
            Self::Guilds => "Guilds",
            Self::Channels => "Channels",
            Self::Messages => "Messages",
            Self::Input => "Input",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Guilds => Self::Channels,
            Self::Channels => Self::Messages,
            Self::Messages => Self::Input,
            Self::Input => Self::Guilds,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Guilds => Self::Input,
            Self::Channels => Self::Guilds,
            Self::Messages => Self::Channels,
            Self::Input => Self::Messages,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
    MockTransport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagePaneView {
    Messages,
    Summary,
}

#[derive(Debug, Clone, Default)]
pub struct SummaryPaneState {
    pub selected_todo: Option<usize>,
    pub in_flight: bool,
    pub last_digest: Option<ChannelDigest>,
}

#[derive(Debug, Clone, Default)]
pub struct DiscordTokenPromptState {
    pub input: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
enum UnreadDividerAnchor {
    StartOfMessages,
    AfterMessage(String),
}

#[expect(clippy::struct_excessive_bools, reason = "independent UI state flags")]
pub struct App {
    pub input_mode: InputMode,
    pub focus: FocusPane,
    pub should_quit: bool,
    pub show_help: bool,
    discord_token_prompt: Option<DiscordTokenPromptState>,
    pub connection_state: ConnectionState,
    pub guilds: Vec<GuildSummary>,
    pub guild_state: ListState,
    pub selected_guild_id: Option<String>,
    pub channels: Vec<ChannelSummary>,
    pub channel_state: ListState,
    pub selected_channel_id: Option<String>,
    pub messages: Vec<MessageRow>,
    pub message_scroll_offset: u16,
    /// Maximum valid scroll offset, computed during render.
    pub max_scroll: u16,
    /// When true, the render loop auto-scrolls to show the newest messages.
    pub at_bottom: bool,
    pub message_pane_view: MessagePaneView,
    pub summary_state: SummaryPaneState,
    pub input_text: String,
    pub current_username: String,
    status_error: Option<(String, Instant)>,
    next_message_id: u64,
    pub store: Option<Rc<Store>>,
    /// True if we tried to restore the session but channels weren't available yet.
    session_restore_pending: bool,
    /// Index into self.messages marking the boundary between read and unread.
    /// Messages at index >= `read_watermark` are considered "unread".
    read_watermark: usize,
    /// Pending anchor used to resolve the "new messages" divider after history loads.
    unread_divider_anchor: Option<UnreadDividerAnchor>,
    /// Message id before which the "new messages" divider should be rendered.
    unread_divider_message_id: Option<String>,
    /// Channels per guild, populated from `GUILD_CREATE` events.
    guild_channels: HashMap<String, Vec<ChannelSummary>>,
    /// User mute preferences from Discord notification settings.
    guild_mute_settings: HashMap<String, GuildMuteSettings>,
    /// Active user-visible async loads for progress rendering.
    active_loads: BTreeSet<LoadScope>,
    /// Tick counter for periodic refresh (every ~10s at 250ms tick rate).
    tick_count: u32,
    pub avatars: AvatarStore,
}

impl App {
    #[cfg_attr(not(test), expect(dead_code, reason = "test helper constructor"))]
    pub fn new() -> Self {
        Self::new_with_avatars(AvatarStore::fallback())
    }

    pub fn new_with_avatars(avatars: AvatarStore) -> Self {
        Self {
            input_mode: InputMode::Normal,
            focus: FocusPane::Guilds,
            should_quit: false,
            show_help: false,
            discord_token_prompt: None,
            connection_state: ConnectionState::MockTransport,
            guilds: Vec::new(),
            guild_state: ListState::default(),
            selected_guild_id: None,
            channels: Vec::new(),
            channel_state: ListState::default(),
            selected_channel_id: None,
            messages: Vec::new(),
            message_scroll_offset: 0,
            max_scroll: 0,
            at_bottom: true,
            message_pane_view: MessagePaneView::Messages,
            summary_state: SummaryPaneState::default(),
            input_text: String::new(),
            current_username: "you".into(),
            session_restore_pending: false,
            status_error: None,
            next_message_id: 1000,
            store: None,
            read_watermark: 0,
            unread_divider_anchor: None,
            unread_divider_message_id: None,
            guild_channels: HashMap::new(),
            guild_mute_settings: HashMap::new(),
            active_loads: BTreeSet::new(),
            tick_count: 0,
            avatars,
        }
    }

    /// Returns effects to execute after the initial load.
    pub fn init_effects() -> Vec<Effect> {
        vec![Effect::LoadGuilds]
    }

    /// Process an action. Returns effects to execute asynchronously.
    #[expect(
        clippy::too_many_lines,
        reason = "central app reducer keeps modal and pane transitions in one match"
    )]
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        // Startup token prompt is modal: only allow quitting, ticks, and resizes.
        if self.discord_token_prompt.is_some() {
            match action {
                Action::Quit => {}
                Action::Tick | Action::Resize { .. } => {
                    return self.tick();
                }
                _ => return Vec::new(),
            }
        }

        // Help popup is modal: only allow closing it, quitting, ticks, and resizes
        if self.show_help {
            match action {
                Action::ToggleHelp | Action::Quit => {}
                Action::Tick | Action::Resize { .. } => {
                    return self.tick();
                }
                _ => return Vec::new(),
            }
        }

        match action {
            Action::Quit => self.should_quit = true,
            Action::Tick => return self.tick(),
            Action::Resize { .. }
            | Action::SubmitDiscordToken
            | Action::CancelDiscordToken
            | Action::ReconnectTransport => {}
            Action::LoadStarted(scope) => {
                self.start_load_scope(scope);
            }
            Action::LoadFailed { scope, message } => {
                self.finish_load_scope(&scope);
                self.set_error(message);
            }
            Action::FocusNext => self.focus = self.focus.next(),
            Action::FocusPrev => self.focus = self.focus.prev(),
            Action::SetFocus(pane) => self.focus = pane,
            Action::MoveUp => self.move_selection(-1),
            Action::MoveDown => self.move_selection(1),
            Action::ScrollUp(n) => {
                self.at_bottom = false;
                self.message_scroll_offset = self.message_scroll_offset.saturating_sub(n);
            }
            Action::ScrollDown(n) => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(n)
                    .min(self.max_scroll);
                if self.message_scroll_offset >= self.max_scroll {
                    self.at_bottom = true;
                }
            }
            Action::JumpTop => self.jump_to_boundary(true),
            Action::JumpBottom => self.jump_to_boundary(false),
            Action::MarkAllRead => self.mark_all_read(),
            Action::RefreshNow => return self.refresh_now(),
            Action::OpenSelected => return self.open_selected(),
            Action::EnterInsert | Action::StartQuickReplyFromSummary => {
                self.input_mode = InputMode::Insert;
                self.focus = FocusPane::Input;
            }
            Action::ExitInsert => {
                self.input_mode = InputMode::Normal;
                self.focus = FocusPane::Messages;
            }
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::ShowMessages | Action::JumpToTodoMessage => {
                self.message_pane_view = MessagePaneView::Messages;
            }
            Action::ShowSummary => self.message_pane_view = MessagePaneView::Summary,
            Action::RequestSummary => return self.handle_request_summary(),
            Action::SummarySelectNextTodo => self.summary_select_next(),
            Action::SummarySelectPrevTodo => self.summary_select_prev(),
            Action::SendCurrentMessage => return self.handle_send(),
            Action::TransportConnecting => self.connection_state = ConnectionState::Connecting,
            Action::TransportConnected { username } => {
                self.finish_load_scope(&LoadScope::StartupConnect);
                if self.guilds.is_empty() {
                    self.start_load_scope(LoadScope::GuildBootstrap);
                }
                self.connection_state = ConnectionState::Connected;
                self.current_username = username;
            }
            Action::TransportDisconnected(reason) => {
                self.finish_load_scope(&LoadScope::StartupConnect);
                self.finish_load_scope(&LoadScope::GuildBootstrap);
                self.connection_state = ConnectionState::Disconnected;
                self.set_error(format!("{reason}. Press c to reconnect."));
            }
            Action::GuildsLoaded(g) => {
                self.finish_load_scope(&LoadScope::GuildBootstrap);
                return self.handle_guilds_loaded(g);
            }
            Action::ReadyData {
                guilds,
                guild_channels,
                guild_mute_settings,
            } => {
                self.finish_load_scope(&LoadScope::GuildBootstrap);
                return self.handle_ready_data(guilds, guild_channels, guild_mute_settings);
            }
            Action::GuildAvailable { guild, channels } => {
                return self.handle_guild_available(guild, channels);
            }
            Action::GuildMuteSettingsUpdated(settings) => {
                self.handle_guild_mute_settings_updated(settings);
            }
            Action::ChannelsLoaded { guild_id, channels } => {
                if let Some(guild_id) = guild_id.as_deref() {
                    self.finish_load_scope(&LoadScope::ChannelList(guild_id.to_string()));
                }
                return self.handle_channels_loaded(guild_id.as_deref(), channels);
            }
            Action::HistoryLoaded {
                messages,
                channel_id,
                ..
            } => {
                self.finish_load_scope(&LoadScope::History(channel_id.clone()));
                return self.handle_history_loaded(&channel_id, messages);
            }
            Action::MessageAppended {
                message,
                channel_hint,
            } => return self.handle_message_appended(message, channel_hint),
            Action::MessagePatched(msg) => self.handle_message_patched(msg),
            Action::MessageRemoved {
                channel_id,
                message_id,
            } => self.handle_message_removed(&channel_id, &message_id),
            Action::SummaryReady(digest) => self.handle_summary_ready(digest),
            Action::SummaryFailed(err) => {
                tracing::warn!("summary failed: {err}");
                self.summary_state.in_flight = false;
                self.set_error(err);
            }
            Action::AvatarLoaded { url, bytes } => self.avatars.store_bytes(url, bytes),
            Action::AvatarFailed(url) => self.avatars.mark_failed(url),
            Action::Error(msg) => self.set_error(msg),
        }

        Vec::new()
    }

    pub fn selected_channel_name(&self) -> Option<&str> {
        let id = self.selected_channel_id.as_deref()?;
        self.channels
            .iter()
            .find(|ch| ch.id == id)
            .map(|ch| ch.name.as_str())
    }

    pub fn selected_channel_kind(&self) -> Option<ChannelKind> {
        let id = self.selected_channel_id.as_deref()?;
        self.channels
            .iter()
            .find(|ch| ch.id == id)
            .map(|ch| ch.kind)
    }

    pub fn selected_guild_muted(&self) -> bool {
        let Some(id) = self.selected_guild_id.as_deref() else {
            return false;
        };
        self.guilds
            .iter()
            .find(|guild| guild.id == id)
            .is_some_and(|guild| guild.muted)
    }

    pub fn selected_channel_directly_muted(&self) -> bool {
        let Some(id) = self.selected_channel_id.as_deref() else {
            return false;
        };
        self.channels
            .iter()
            .find(|channel| channel.id == id)
            .is_some_and(|channel| channel.muted)
    }

    pub fn selected_guild_name(&self) -> Option<&str> {
        let id = self.selected_guild_id.as_deref()?;
        self.guilds
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.name.as_str())
    }

    pub fn status_error(&self) -> Option<&str> {
        self.status_error.as_ref().map(|(msg, _)| msg.as_str())
    }

    pub fn active_load_label(&self) -> Option<&'static str> {
        self.prioritized_load_scope().map(LoadScope::status_label)
    }

    pub fn active_load_progress_bar(&self) -> Option<String> {
        self.prioritized_load_scope()
            .map(|_| ghostty_progress_bar(self.tick_count))
    }

    pub fn guild_pane_loading(&self) -> bool {
        matches!(
            self.prioritized_load_scope(),
            Some(LoadScope::GuildBootstrap)
        )
    }

    pub fn guild_pane_loading_bar(&self) -> Option<String> {
        self.guild_pane_loading()
            .then(|| ghostty_progress_bar(self.tick_count))
    }

    pub fn channels_pane_loading(&self) -> bool {
        let Some(guild_id) = self.selected_guild_id.as_deref() else {
            return false;
        };
        matches!(
            self.prioritized_load_scope(),
            Some(LoadScope::ChannelList(active_guild_id)) if active_guild_id == guild_id
        )
    }

    pub fn channels_pane_loading_bar(&self) -> Option<String> {
        self.channels_pane_loading()
            .then(|| ghostty_progress_bar(self.tick_count))
    }

    pub fn messages_pane_loading(&self) -> bool {
        let Some(channel_id) = self.selected_channel_id.as_deref() else {
            return false;
        };
        matches!(
            self.prioritized_load_scope(),
            Some(LoadScope::History(active_channel_id)) if active_channel_id == channel_id
        )
    }

    pub fn messages_pane_loading_bar(&self) -> Option<String> {
        self.messages_pane_loading()
            .then(|| ghostty_progress_bar(self.tick_count))
    }

    pub fn unread_divider_message_id(&self) -> Option<&str> {
        self.unread_divider_message_id.as_deref()
    }

    pub fn discord_token_prompt(&self) -> Option<&DiscordTokenPromptState> {
        self.discord_token_prompt.as_ref()
    }

    pub fn has_discord_token_prompt(&self) -> bool {
        self.discord_token_prompt.is_some()
    }

    pub fn show_discord_token_prompt(&mut self) {
        self.discord_token_prompt = Some(DiscordTokenPromptState::default());
        self.input_mode = InputMode::Normal;
        self.show_help = false;
        self.connection_state = ConnectionState::Disconnected;
    }

    #[cfg_attr(
        not(feature = "experimental-discord"),
        expect(
            dead_code,
            reason = "startup token prompt is only compiled into Discord builds"
        )
    )]
    pub fn dismiss_discord_token_prompt(&mut self) {
        self.discord_token_prompt = None;
    }

    pub fn push_discord_token_prompt_char(&mut self, ch: char) {
        let Some(prompt) = self.discord_token_prompt.as_mut() else {
            return;
        };
        prompt.input.push(ch);
        prompt.error = None;
    }

    #[cfg_attr(
        not(feature = "experimental-discord"),
        expect(
            dead_code,
            reason = "startup token prompt is only compiled into Discord builds"
        )
    )]
    pub fn pop_discord_token_prompt_char(&mut self) {
        let Some(prompt) = self.discord_token_prompt.as_mut() else {
            return;
        };
        prompt.input.pop();
        prompt.error = None;
    }

    #[cfg_attr(
        not(feature = "experimental-discord"),
        expect(
            dead_code,
            reason = "startup token prompt is only compiled into Discord builds"
        )
    )]
    pub fn set_discord_token_prompt_error(&mut self, error: String) {
        let Some(prompt) = self.discord_token_prompt.as_mut() else {
            return;
        };
        prompt.error = Some(error);
    }

    pub fn take_discord_token_prompt_input(&mut self) -> Option<String> {
        let prompt = self.discord_token_prompt.as_mut()?;
        let input = std::mem::take(&mut prompt.input);
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            prompt.error = Some("Discord token is required".into());
            return None;
        }
        prompt.error = None;
        Some(trimmed)
    }

    fn request_avatar_effects<'a>(
        &mut self,
        urls: impl IntoIterator<Item = &'a str>,
    ) -> Vec<Effect> {
        urls.into_iter()
            .filter(|url| !url.is_empty())
            .filter(|url| self.avatars.request(url))
            .map(|url| Effect::FetchAvatar {
                url: url.to_string(),
            })
            .collect()
    }

    fn set_error(&mut self, msg: String) {
        self.status_error = Some((msg, Instant::now()));
    }

    fn apply_local_read_state_to_channels(store: &Store, channels: &mut [ChannelSummary]) -> u32 {
        let mut cleared = 0u32;

        for channel in channels {
            if !channel.unread {
                continue;
            }
            let Some(last_message_id) = &channel.last_message_id else {
                continue;
            };

            let is_read = store.last_read_message(&channel.id).is_some_and(|read_id| {
                match (read_id.parse::<u64>(), last_message_id.parse::<u64>()) {
                    (Ok(read_id), Ok(last_message_id)) => read_id >= last_message_id,
                    _ => false,
                }
            });
            if !is_read {
                continue;
            }

            channel.unread = false;
            channel.unread_count = 0;
            cleared += 1;
        }

        cleared
    }

    pub(crate) fn start_load_scope(&mut self, scope: LoadScope) {
        self.active_loads.insert(scope);
    }

    pub(crate) fn finish_load_scope(&mut self, scope: &LoadScope) {
        self.active_loads.remove(scope);
    }

    // Display priority is independent of BTreeSet ordering.
    fn prioritized_load_scope(&self) -> Option<&LoadScope> {
        self.active_loads
            .iter()
            .max_by_key(|scope| scope.display_priority())
    }

    fn ensure_direct_messages_guild(guilds: &mut Vec<GuildSummary>) {
        if guilds
            .iter()
            .any(|guild| guild.id == DIRECT_MESSAGES_GUILD_ID)
        {
            return;
        }

        guilds.push(GuildSummary {
            id: DIRECT_MESSAGES_GUILD_ID.to_string(),
            name: DIRECT_MESSAGES_GUILD_NAME.to_string(),
            muted: false,
            unread: false,
            unread_count: 0,
            avatar_url: None,
        });
    }

    fn maybe_load_direct_messages(&self, effects: &mut Vec<Effect>) {
        if !self
            .guilds
            .iter()
            .any(|guild| guild.id == DIRECT_MESSAGES_GUILD_ID)
            || self.guild_channels.contains_key(DIRECT_MESSAGES_GUILD_ID)
            || effects.iter().any(|effect| {
                matches!(
                    effect,
                    Effect::LoadChannels { guild_id } if guild_id == DIRECT_MESSAGES_GUILD_ID
                )
            })
        {
            return;
        }

        effects.push(Effect::LoadChannels {
            guild_id: DIRECT_MESSAGES_GUILD_ID.to_string(),
        });
    }

    fn sync_guild_unread_from_channels(&mut self, guild_id: &str) {
        let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) else {
            return;
        };
        let Some(channels) = self.guild_channels.get(guild_id) else {
            return;
        };
        let guild_muted = guild.muted;

        let total: u32 = channels
            .iter()
            .filter(|channel| channel.shows_unread_in_guild_rollup(guild_muted))
            .map(|channel| channel.unread_count.max(1))
            .sum();
        guild.unread = total > 0;
        guild.unread_count = total;
    }

    fn sort_visible_channels(&mut self) {
        sort_channels_for_sidebar(&mut self.channels);
    }

    fn apply_guild_mute_settings_to_channels(
        guild_id: &str,
        channels: &mut [ChannelSummary],
        guild_mute_settings: &HashMap<String, GuildMuteSettings>,
    ) {
        let Some(settings) = guild_mute_settings.get(guild_id) else {
            for channel in channels {
                channel.muted = false;
            }
            return;
        };

        for channel in channels {
            channel.muted = settings
                .channel_overrides
                .get(&channel.id)
                .is_some_and(|override_settings| override_settings.muted);
        }
    }

    fn channel_is_read(store: Option<&Store>, channel: &ChannelSummary) -> bool {
        let Some(store) = store else {
            return false;
        };
        let Some(last_message_id) = channel.last_message_id.as_deref() else {
            return false;
        };
        store.last_read_message(&channel.id).is_some_and(|read_id| {
            match (read_id.parse::<u64>(), last_message_id.parse::<u64>()) {
                (Ok(read_id), Ok(last_message_id)) => read_id >= last_message_id,
                _ => false,
            }
        })
    }

    fn find_channel(&self, channel_id: &str) -> Option<&ChannelSummary> {
        self.channels
            .iter()
            .find(|channel| channel.id == channel_id)
            .or_else(|| {
                self.guild_channels
                    .values()
                    .flat_map(|channels| channels.iter())
                    .find(|channel| channel.id == channel_id)
            })
    }

    fn log_channel_debug_state(
        &self,
        channel_id: &str,
        channel: Option<&ChannelSummary>,
        reason: &str,
        loaded_messages: Option<usize>,
    ) {
        let stored_read = self
            .store
            .as_deref()
            .and_then(|store| store.last_read_message(channel_id));

        if let Some(channel) = channel {
            tracing::debug!(
                reason,
                channel_id = %channel.id,
                channel_name = %channel.name,
                kind = ?channel.kind,
                guild_id = ?channel.guild_id,
                unread = channel.unread,
                unread_count = channel.unread_count,
                last_message_id = ?channel.last_message_id,
                stored_read = ?stored_read,
                selected = self.selected_channel_id.as_deref() == Some(channel_id),
                loaded_messages = ?loaded_messages,
                "channel debug state"
            );
        } else {
            tracing::debug!(
                reason,
                channel_id,
                stored_read = ?stored_read,
                selected = self.selected_channel_id.as_deref() == Some(channel_id),
                loaded_messages = ?loaded_messages,
                "channel debug state missing from caches"
            );
        }
    }

    fn snowflake_eq(left: &str, right: &str) -> bool {
        match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left == right,
            _ => left == right,
        }
    }

    fn message_id_advanced(previous: Option<&str>, current: Option<&str>) -> bool {
        match (
            previous.and_then(|id| id.parse::<u64>().ok()),
            current.and_then(|id| id.parse::<u64>().ok()),
        ) {
            (Some(previous), Some(current)) => current > previous,
            (None, Some(_)) => true,
            _ => false,
        }
    }

    fn resolve_unread_divider_message_id(
        messages: &[MessageRow],
        anchor: Option<&UnreadDividerAnchor>,
    ) -> Option<String> {
        let anchor = anchor?;
        match anchor {
            UnreadDividerAnchor::StartOfMessages => {
                messages.first().map(|message| message.id.clone())
            }
            UnreadDividerAnchor::AfterMessage(read_id) => messages
                .iter()
                .position(|message| Self::snowflake_eq(message.id.as_str(), read_id.as_str()))
                .and_then(|idx| messages.get(idx.saturating_add(1)))
                // If the read anchor is older than the loaded history window, every visible
                // message is still newer than the last acknowledged message.
                .or_else(|| messages.first())
                .map(|message| message.id.clone()),
        }
    }

    fn merge_refreshed_channels(
        &self,
        fresh_channels: &mut Vec<ChannelSummary>,
        previous_channels: Option<&[ChannelSummary]>,
    ) {
        let store = self.store.as_deref();

        if let Some(previous_channels) = previous_channels {
            let previous_by_id: HashMap<&str, &ChannelSummary> = previous_channels
                .iter()
                .map(|channel| (channel.id.as_str(), channel))
                .collect();
            for fresh in fresh_channels.iter_mut() {
                let Some(previous) = previous_by_id.get(fresh.id.as_str()).copied() else {
                    continue;
                };

                if fresh.last_message_id.is_none() {
                    fresh.last_message_id.clone_from(&previous.last_message_id);
                }

                if self.selected_channel_id.as_deref() == Some(fresh.id.as_str()) {
                    continue;
                }

                if Self::channel_is_read(store, fresh) {
                    fresh.unread = false;
                    fresh.unread_count = 0;
                    continue;
                }

                let message_advanced = Self::message_id_advanced(
                    previous.last_message_id.as_deref(),
                    fresh.last_message_id.as_deref(),
                );

                if fresh.unread || previous.unread || message_advanced {
                    fresh.unread = true;
                    if fresh.unread_count == 0 {
                        fresh.unread_count = if message_advanced {
                            if previous.unread {
                                previous.unread_count.max(1).saturating_add(1)
                            } else {
                                1
                            }
                        } else {
                            previous.unread_count.max(1)
                        };
                    }
                }
            }

            let fresh_ids: HashSet<String> = fresh_channels
                .iter()
                .map(|channel| channel.id.clone())
                .collect();
            for previous in previous_channels {
                let keep_missing = previous.unread
                    || self.selected_channel_id.as_deref() == Some(previous.id.as_str());
                if keep_missing && !fresh_ids.contains(previous.id.as_str()) {
                    fresh_channels.push(previous.clone());
                }
            }
        }

        sort_channels_for_sidebar(fresh_channels);
    }

    fn apply_guild_mute_settings(
        guilds: &mut [GuildSummary],
        guild_channels: &mut HashMap<String, Vec<ChannelSummary>>,
        guild_mute_settings: &HashMap<String, GuildMuteSettings>,
    ) {
        for guild in guilds.iter_mut() {
            guild.muted = guild_mute_settings
                .get(&guild.id)
                .is_some_and(|settings| settings.muted);
        }

        for (guild_id, channels) in guild_channels.iter_mut() {
            Self::apply_guild_mute_settings_to_channels(guild_id, channels, guild_mute_settings);
        }
    }

    fn prepare_loaded_channels(&self, guild_id: &str, channels: &mut [ChannelSummary]) {
        Self::apply_guild_mute_settings_to_channels(guild_id, channels, &self.guild_mute_settings);
        if let Some(store) = self.store.as_deref() {
            let _ = Self::apply_local_read_state_to_channels(store, channels);
        }
    }

    fn update_visible_channels_mute_state(&mut self) {
        let Some(guild_id) = self.selected_guild_id.as_deref() else {
            return;
        };
        Self::apply_guild_mute_settings_to_channels(
            guild_id,
            &mut self.channels,
            &self.guild_mute_settings,
        );
    }

    fn restore_selected_channel_from_store(&mut self) -> Vec<Effect> {
        let Some(store) = &self.store else {
            self.session_restore_pending = false;
            return Vec::new();
        };

        let guild_id = self.selected_guild_id.clone().unwrap_or_default();
        self.session_restore_pending = false;

        let Some(channel_id) = store.get_session(store::KEY_LAST_CHANNEL) else {
            tracing::info!("restored session: guild={guild_id} (no channel restored)");
            return Vec::new();
        };

        let Some(ch_idx) = self
            .channels
            .iter()
            .position(|channel| channel.id == channel_id)
        else {
            tracing::info!(
                "restored session: guild={guild_id} (stored channel missing: {channel_id})"
            );
            return Vec::new();
        };

        self.channel_state.select(Some(ch_idx));
        self.selected_channel_id = Some(channel_id.clone());
        self.focus = FocusPane::Messages;
        tracing::info!("restored session: guild={guild_id}, channel={channel_id}");
        vec![Effect::LoadHistory { channel_id }]
    }

    /// Cross-reference gateway unread state with our local DB.
    /// If we've locally marked a channel as read (via R or opening it),
    /// and the DB's read position >= the channel's `last_message_id`,
    /// clear the unread flag.
    fn apply_local_read_state(&mut self) {
        let Some(store) = &self.store else { return };

        let mut cleared = 0u32;
        for channels in self.guild_channels.values_mut() {
            cleared += Self::apply_local_read_state_to_channels(store, channels);
        }

        // Recalculate guild unread from channel state
        let guild_ids: Vec<String> = self.guilds.iter().map(|guild| guild.id.clone()).collect();
        for guild_id in guild_ids {
            self.sync_guild_unread_from_channels(&guild_id);
        }

        if cleared > 0 {
            tracing::info!("applied local read state: cleared {cleared} channels");
        }
    }

    /// Restore last guild/channel selection from the store.
    fn restore_session(&mut self) -> Vec<Effect> {
        let Some(store) = &self.store else {
            return Vec::new();
        };

        let Some(guild_id) = store.get_session(store::KEY_LAST_GUILD) else {
            return Vec::new();
        };

        let Some(idx) = self.guilds.iter().position(|g| g.id == guild_id) else {
            return Vec::new();
        };

        self.guild_state.select(Some(idx));
        self.selected_guild_id = Some(guild_id.clone());

        // Load channels for this guild (may be empty if guild arrived via lazy load)
        if let Some(channels) = self.guild_channels.get(&guild_id) {
            self.channels = channels.clone();
            self.channel_state = ListState::default();
            if let Some(ch_idx) = self.first_selectable_channel_index() {
                self.channel_state.select(Some(ch_idx));
            }
            return self.restore_selected_channel_from_store();
        }

        if !self.guild_channels.contains_key(&guild_id) {
            tracing::info!("session restore pending: channels for {guild_id} not yet available");
            self.session_restore_pending = true;
        }
        tracing::info!("restored session: guild={guild_id} (no channel restored)");
        Vec::new()
    }

    /// Save session state before quitting.
    pub fn save_on_quit(&self) {
        let Some(store) = &self.store else { return };
        if let Some(guild_id) = &self.selected_guild_id {
            store.set_session(store::KEY_LAST_GUILD, guild_id);
        }
        if let Some(channel_id) = &self.selected_channel_id {
            store.set_session(store::KEY_LAST_CHANNEL, channel_id);
            // Mark the last visible message as read
            if let Some(last_msg) = self.messages.last() {
                store.mark_read(channel_id, &last_msg.id);
            }
        }
    }

    /// Clear all unread indicators and persist read positions to DB.
    fn mark_all_read(&mut self) {
        for guild in &mut self.guilds {
            guild.unread = false;
            guild.unread_count = 0;
        }
        for ch in &mut self.channels {
            ch.unread = false;
            ch.unread_count = 0;
        }
        // Clear and persist for all cached guild channels
        for channels in self.guild_channels.values_mut() {
            for ch in channels.iter_mut() {
                if ch.unread {
                    // Persist the last_message_id as our read position
                    if let (Some(store), Some(msg_id)) = (&self.store, &ch.last_message_id) {
                        store.mark_read(&ch.id, msg_id);
                    }
                }
                ch.unread = false;
                ch.unread_count = 0;
            }
        }
        // Also persist for active channel's last visible message
        if let (Some(store), Some(ch_id), Some(last_msg)) =
            (&self.store, &self.selected_channel_id, self.messages.last())
        {
            store.mark_read(ch_id, &last_msg.id);
        }
        self.read_watermark = self.messages.len();
        self.unread_divider_anchor = None;
        self.unread_divider_message_id = None;
    }

    fn tick(&mut self) -> Vec<Effect> {
        if self
            .status_error
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs() >= 5)
        {
            self.status_error = None;
        }

        self.tick_count += 1;
        // Live Discord already receives gateway updates; periodic REST refreshes can still race
        // with fresher gateway-delivered state, so keep auto-refresh scoped to the mock transport.
        if self.connection_state == ConnectionState::MockTransport
            && self.tick_count.is_multiple_of(40)
        {
            return self.refresh_effects();
        }

        Vec::new()
    }

    fn refresh_now(&mut self) -> Vec<Effect> {
        let effects = self.refresh_effects();
        if effects.is_empty() {
            let message = match self.connection_state {
                ConnectionState::Connecting | ConnectionState::Reconnecting => "Still connecting",
                ConnectionState::Disconnected => "Connect first to refresh",
                ConnectionState::Connected | ConnectionState::MockTransport => {
                    if self.selected_guild_id.is_none() && self.selected_channel_id.is_none() {
                        "Select a guild or channel to refresh"
                    } else {
                        "Refresh already in progress"
                    }
                }
            };
            self.set_error(message.into());
        }
        effects
    }

    fn refresh_effects(&self) -> Vec<Effect> {
        if matches!(
            self.connection_state,
            ConnectionState::Connecting | ConnectionState::Disconnected
        ) {
            return Vec::new();
        }

        let mut effects = Vec::new();

        if let Some(guild_id) = self.selected_guild_id.clone() {
            let scope = LoadScope::ChannelList(guild_id.clone());
            if !self.active_loads.contains(&scope) {
                effects.push(Effect::LoadChannels { guild_id });
            }
        }

        if let Some(channel_id) = self.selected_channel_id.clone() {
            let scope = LoadScope::History(channel_id.clone());
            if !self.active_loads.contains(&scope) {
                effects.push(Effect::LoadHistory { channel_id });
            }
        } else if self.guilds.is_empty()
            && !self.active_loads.contains(&LoadScope::GuildBootstrap)
            && self.connection_state == ConnectionState::MockTransport
        {
            effects.push(Effect::LoadGuilds);
        }

        effects
    }

    fn handle_request_summary(&mut self) -> Vec<Effect> {
        let Some(channel_id) = self.selected_channel_id.clone() else {
            self.set_error("No channel selected".into());
            return Vec::new();
        };

        // Only summarize unread messages (after the read watermark).
        // Fall back to all messages if nothing is unread.
        let unread = if self.read_watermark < self.messages.len() {
            self.messages[self.read_watermark..].to_vec()
        } else {
            self.messages.clone()
        };

        if unread.is_empty() {
            self.set_error("No messages to summarize".into());
            return Vec::new();
        }

        let channel_name = self
            .selected_channel_name()
            .unwrap_or("unknown")
            .to_string();

        self.summary_state.in_flight = true;
        self.message_pane_view = MessagePaneView::Summary;

        vec![Effect::SummarizeChannel {
            channel_id,
            channel_name,
            messages: unread,
            user_name: self.current_username.clone(),
        }]
    }

    fn summary_select_next(&mut self) {
        if let Some(digest) = &self.summary_state.last_digest {
            let max = digest.todos.len().saturating_sub(1);
            self.summary_state.selected_todo = Some(
                self.summary_state
                    .selected_todo
                    .map_or(0, |i| i.saturating_add(1).min(max)),
            );
        }
    }

    fn summary_select_prev(&mut self) {
        if self.summary_state.last_digest.is_some() {
            self.summary_state.selected_todo = Some(
                self.summary_state
                    .selected_todo
                    .map_or(0, |i| i.saturating_sub(1)),
            );
        }
    }

    fn handle_send(&mut self) -> Vec<Effect> {
        if self.input_text.trim().is_empty() {
            return Vec::new();
        }
        let Some(channel_id) = self.selected_channel_id.clone() else {
            self.set_error("No channel selected".into());
            return Vec::new();
        };
        let content = std::mem::take(&mut self.input_text);

        // For mock transport, append locally since there's no gateway echo.
        // For live transport, the gateway MESSAGE_CREATE echo will add it.
        if self.connection_state == ConnectionState::MockTransport {
            let is_continuation = self.messages.last().is_some_and(|m| m.author == "you");
            let msg = MessageRow {
                id: format!("local_{}", self.next_message_id),
                channel_id: channel_id.clone(),
                author: "you".into(),
                author_avatar_url: None,
                content: content.clone(),
                attachments: Vec::new(),
                timestamp: chrono::Local::now().format("%H:%M").to_string(),
                edited: false,
                is_continuation,
            };
            self.next_message_id += 1;
            self.messages.push(msg);
        }

        self.at_bottom = true;
        self.input_mode = InputMode::Normal;
        self.focus = FocusPane::Messages;

        vec![Effect::SendMessage {
            channel_id,
            content,
        }]
    }

    fn handle_guilds_loaded(&mut self, guilds: Vec<GuildSummary>) -> Vec<Effect> {
        let mut guilds = guilds;
        Self::ensure_direct_messages_guild(&mut guilds);
        Self::apply_guild_mute_settings(
            &mut guilds,
            &mut self.guild_channels,
            &self.guild_mute_settings,
        );
        let mut effects = self.request_avatar_effects(
            guilds
                .iter()
                .filter_map(|guild| guild.avatar_url.as_deref()),
        );
        self.guilds = guilds;
        if !self.guilds.is_empty() && self.guild_state.selected().is_none() {
            self.guild_state.select(Some(0));
        }
        let mut restore = self.restore_session();
        effects.append(&mut restore);
        self.maybe_load_direct_messages(&mut effects);
        effects
    }

    fn handle_ready_data(
        &mut self,
        guilds: Vec<GuildSummary>,
        channels: HashMap<String, Vec<ChannelSummary>>,
        guild_mute_settings: HashMap<String, GuildMuteSettings>,
    ) -> Vec<Effect> {
        let mut guilds = guilds;
        let mut channels = channels;
        Self::ensure_direct_messages_guild(&mut guilds);
        self.guild_mute_settings = guild_mute_settings;
        Self::apply_guild_mute_settings(&mut guilds, &mut channels, &self.guild_mute_settings);
        let mut effects = self.request_avatar_effects(
            guilds
                .iter()
                .filter_map(|guild| guild.avatar_url.as_deref()),
        );
        self.guilds = guilds;
        self.guild_channels = channels;
        // Override gateway unread state with our local DB read positions
        self.apply_local_read_state();
        if !self.guilds.is_empty() && self.guild_state.selected().is_none() {
            self.guild_state.select(Some(0));
        }
        let mut restore_effects = self.restore_session();
        effects.append(&mut restore_effects);
        self.maybe_load_direct_messages(&mut effects);
        effects
    }

    fn handle_guild_available(
        &mut self,
        mut guild: GuildSummary,
        mut channels: Vec<ChannelSummary>,
    ) -> Vec<Effect> {
        guild.muted = self
            .guild_mute_settings
            .get(&guild.id)
            .is_some_and(|settings| settings.muted);
        self.prepare_loaded_channels(&guild.id, &mut channels);
        let effects = self.request_avatar_effects(guild.avatar_url.as_deref());
        // Store channels for this guild
        self.guild_channels.insert(guild.id.clone(), channels);

        // Add guild if not already present
        let guild_id = guild.id.clone();
        if !self.guilds.iter().any(|g| g.id == guild_id) {
            self.guilds.push(guild);
            if self.guilds.len() == 1 {
                self.guild_state.select(Some(0));
            }
        }
        self.sync_guild_unread_from_channels(&guild_id);
        // Retry pending session restore now that new channels are available
        if self.session_restore_pending {
            let mut restore = self.restore_session();
            if !restore.is_empty() {
                self.session_restore_pending = false;
                let mut effects = effects;
                effects.append(&mut restore);
                return effects;
            }
        }
        effects
    }

    fn handle_guild_mute_settings_updated(&mut self, settings: GuildMuteSettings) {
        let guild_id = settings.guild_id.clone();
        self.guild_mute_settings.insert(guild_id.clone(), settings);
        let guild_muted = self
            .guild_mute_settings
            .get(&guild_id)
            .is_some_and(|stored| stored.muted);

        if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) {
            guild.muted = guild_muted;
        }
        if let Some(channels) = self.guild_channels.get_mut(&guild_id) {
            Self::apply_guild_mute_settings_to_channels(
                &guild_id,
                channels,
                &self.guild_mute_settings,
            );
        }
        if self.selected_guild_id.as_deref() == Some(guild_id.as_str()) {
            self.update_visible_channels_mute_state();
        }
        self.sync_guild_unread_from_channels(&guild_id);
    }

    fn handle_channels_loaded(
        &mut self,
        guild_id: Option<&str>,
        mut channels: Vec<ChannelSummary>,
    ) -> Vec<Effect> {
        let previous_channels = guild_id.and_then(|gid| self.guild_channels.get(gid).cloned());
        let previous_selected_channel_id = self.selected_channel_id.clone();

        // Persist into the per-guild cache so restore_session and unread tracking work
        if let Some(gid) = guild_id {
            self.prepare_loaded_channels(gid, &mut channels);
            self.merge_refreshed_channels(&mut channels, previous_channels.as_deref());
            self.guild_channels
                .insert(gid.to_string(), channels.clone());
            self.sync_guild_unread_from_channels(gid);
        }

        if guild_id == Some(DIRECT_MESSAGES_GUILD_ID) {
            for channel in channels
                .iter()
                .filter(|channel| channel.unread || channel.last_message_id.is_some())
            {
                let stored_read = self
                    .store
                    .as_deref()
                    .and_then(|store| store.last_read_message(&channel.id));
                tracing::debug!(
                    reason = "dm_channels_loaded",
                    channel_id = %channel.id,
                    channel_name = %channel.name,
                    kind = ?channel.kind,
                    guild_id = ?channel.guild_id,
                    unread = channel.unread,
                    unread_count = channel.unread_count,
                    last_message_id = ?channel.last_message_id,
                    stored_read = ?stored_read,
                    "loaded DM channel summary"
                );
            }
        }

        if guild_id.is_some() && guild_id != self.selected_guild_id.as_deref() {
            return Vec::new();
        }

        self.channels = channels;
        self.channel_state = ListState::default();
        if let Some(selected_channel_id) = previous_selected_channel_id.as_deref()
            && let Some(idx) = self
                .channels
                .iter()
                .position(|channel| channel.id == selected_channel_id)
        {
            self.channel_state.select(Some(idx));
        } else if let Some(idx) = self.first_selectable_channel_index() {
            self.channel_state.select(Some(idx));
        }

        if self.session_restore_pending && guild_id == self.selected_guild_id.as_deref() {
            return self.restore_selected_channel_from_store();
        }

        Vec::new()
    }

    fn handle_history_loaded(
        &mut self,
        channel_id: &str,
        messages: Vec<MessageRow>,
    ) -> Vec<Effect> {
        if self.selected_channel_id.as_deref() == Some(channel_id) {
            let current_channel = self.find_channel(channel_id);
            let suspicious_empty_history = current_channel.is_some_and(|channel| {
                channel.last_message_id.is_some() || channel.unread || channel.unread_count > 0
            }) || self.unread_divider_anchor.is_some()
                || self.unread_divider_message_id.is_some();
            self.log_channel_debug_state(
                channel_id,
                current_channel,
                "history_loaded_before_apply",
                Some(messages.len()),
            );
            if messages.is_empty() && suspicious_empty_history {
                tracing::warn!(
                    channel_id,
                    unread_divider_anchor = ?self.unread_divider_anchor,
                    unread_divider_message_id = ?self.unread_divider_message_id,
                    "selected channel history loaded empty; read state will not advance"
                );
            }
            let effects = self.request_avatar_effects(
                messages
                    .iter()
                    .filter_map(|msg| msg.author_avatar_url.as_deref()),
            );
            let mut attachment_effects = self.request_avatar_effects(
                messages
                    .iter()
                    .flat_map(|msg| msg.attachments.iter())
                    .filter(|attachment| attachment.is_image)
                    .map(|attachment| attachment.url.as_str()),
            );
            let had_messages = !self.messages.is_empty();
            let preserved_divider = self
                .unread_divider_message_id
                .as_ref()
                .filter(|message_id| messages.iter().any(|message| &message.id == *message_id))
                .cloned();
            let resolved_divider = preserved_divider.or_else(|| {
                Self::resolve_unread_divider_message_id(
                    &messages,
                    self.unread_divider_anchor.as_ref(),
                )
            });
            self.unread_divider_message_id.clone_from(&resolved_divider);
            self.unread_divider_anchor = None;
            self.read_watermark = resolved_divider
                .as_ref()
                .and_then(|message_id| {
                    messages
                        .iter()
                        .position(|message| &message.id == message_id)
                })
                .unwrap_or(messages.len());

            // Persist the last message ID as our read position (only if changed)
            if let Some(last) = messages.last()
                && self.messages.last().is_none_or(|prev| prev.id != last.id)
                && let Some(store) = &self.store
            {
                store.mark_read(channel_id, &last.id);
            }

            self.messages = messages;
            if !had_messages {
                self.at_bottom = true;
            }
            self.log_channel_debug_state(
                channel_id,
                self.find_channel(channel_id),
                "history_loaded_after_apply",
                Some(self.messages.len()),
            );
            let mut effects = effects;
            effects.append(&mut attachment_effects);
            return effects;
        }
        Vec::new()
    }

    fn handle_message_appended(
        &mut self,
        msg: MessageRow,
        channel_hint: Option<ChannelSummary>,
    ) -> Vec<Effect> {
        if self.selected_channel_id.as_deref() == Some(&msg.channel_id) {
            let effects = self.request_avatar_effects(msg.author_avatar_url.as_deref());
            let mut attachment_effects = self.request_avatar_effects(
                msg.attachments
                    .iter()
                    .filter(|attachment| attachment.is_image)
                    .map(|attachment| attachment.url.as_str()),
            );
            self.messages.push(msg);
            let mut effects = effects;
            effects.append(&mut attachment_effects);
            return effects;
        }

        // Message is for a non-active channel — mark it as unread and advance last_message_id
        let channel_id = &msg.channel_id;
        let msg_id = msg.id.clone();
        let mut affected_guild_id: Option<String> = None;
        let mut matched_channel = false;
        for channels in self.guild_channels.values_mut() {
            if let Some(ch) = channels.iter_mut().find(|c| c.id == *channel_id) {
                mark_channel_unread(ch, &msg_id);
                affected_guild_id.clone_from(&ch.guild_id);
                matched_channel = true;
                break;
            }
        }
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == *channel_id) {
            mark_channel_unread(ch, &msg_id);
            matched_channel = true;
        }

        if !matched_channel
            && let Some(mut hinted_channel) = channel_hint
            && let Some(guild_id) = hinted_channel.guild_id.clone()
        {
            Self::apply_guild_mute_settings_to_channels(
                &guild_id,
                std::slice::from_mut(&mut hinted_channel),
                &self.guild_mute_settings,
            );
            mark_channel_unread(&mut hinted_channel, &msg_id);
            affected_guild_id = Some(guild_id.clone());
            let cached_channels = self.guild_channels.entry(guild_id.clone()).or_default();
            if !cached_channels
                .iter()
                .any(|channel| channel.id == hinted_channel.id)
            {
                cached_channels.push(hinted_channel.clone());
            }

            if self.selected_guild_id.as_deref() == Some(guild_id.as_str())
                && !self
                    .channels
                    .iter()
                    .any(|channel| channel.id == hinted_channel.id)
            {
                self.channels.push(hinted_channel);
                self.sort_visible_channels();
            }
        }

        // Mark the parent guild as unread, respecting mute visibility rules.
        if let Some(guild_id) = affected_guild_id {
            self.sync_guild_unread_from_channels(&guild_id);
        }

        Vec::new()
    }

    fn handle_message_patched(&mut self, msg: MessageRow) {
        if let Some(existing) = self.messages.iter_mut().find(|m| m.id == msg.id) {
            *existing = msg;
        }
    }

    fn handle_message_removed(&mut self, channel_id: &str, message_id: &str) {
        self.messages.retain(|m| m.id != message_id);
        // Decrement unread if the deleted message was in a background channel
        if self.selected_channel_id.as_deref() != Some(channel_id) {
            let mut affected_guild_id: Option<String> = None;
            for channels in self.guild_channels.values_mut() {
                if let Some(ch) = channels.iter_mut().find(|c| c.id == channel_id) {
                    ch.unread_count = ch.unread_count.saturating_sub(1);
                    if ch.unread_count == 0 {
                        ch.unread = false;
                    }
                    affected_guild_id.clone_from(&ch.guild_id);
                    break;
                }
            }
            if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel_id) {
                ch.unread_count = ch.unread_count.saturating_sub(1);
                if ch.unread_count == 0 {
                    ch.unread = false;
                }
            }
            if let Some(guild_id) = affected_guild_id {
                self.sync_guild_unread_from_channels(&guild_id);
            }
        }
    }

    fn handle_summary_ready(&mut self, digest: ChannelDigest) {
        // Reject stale summary for a different channel
        if self.selected_channel_id.as_deref() != Some(&digest.channel_id) {
            self.summary_state.in_flight = false;
            return;
        }
        self.summary_state.in_flight = false;
        self.summary_state.selected_todo = if digest.todos.is_empty() {
            None
        } else {
            Some(0)
        };
        self.summary_state.last_digest = Some(digest);
    }

    fn move_selection(&mut self, delta: i32) {
        match self.focus {
            FocusPane::Guilds => {
                move_list_selection(&mut self.guild_state, self.guilds.len(), delta);
                self.sync_guild_list_offset();
            }
            FocusPane::Channels => {
                self.move_channel_selection(delta);
            }
            FocusPane::Messages => {
                if self.message_pane_view == MessagePaneView::Summary {
                    if delta > 0 {
                        self.summary_select_next();
                    } else {
                        self.summary_select_prev();
                    }
                } else if delta < 0 {
                    self.at_bottom = false;
                    self.message_scroll_offset = self.message_scroll_offset.saturating_sub(1);
                } else {
                    self.message_scroll_offset = self
                        .message_scroll_offset
                        .saturating_add(1)
                        .min(self.max_scroll);
                    // Re-enable auto-follow if we've scrolled back to the bottom
                    if self.message_scroll_offset >= self.max_scroll {
                        self.at_bottom = true;
                    }
                }
            }
            FocusPane::Input => {}
        }
    }

    fn jump_to_boundary(&mut self, to_top: bool) {
        match self.focus {
            FocusPane::Guilds if !self.guilds.is_empty() => {
                let idx = if to_top { 0 } else { self.guilds.len() - 1 };
                self.guild_state.select(Some(idx));
                self.sync_guild_list_offset();
            }
            FocusPane::Channels if !self.channels.is_empty() => {
                let indices = self.selectable_channel_indices();
                if let Some(idx) = if to_top {
                    indices.first().copied()
                } else {
                    indices.last().copied()
                } {
                    self.channel_state.select(Some(idx));
                }
            }
            FocusPane::Messages if to_top => {
                self.message_scroll_offset = 0;
                self.at_bottom = false;
            }
            FocusPane::Messages => self.at_bottom = true,
            _ => {}
        }
    }

    fn open_selected(&mut self) -> Vec<Effect> {
        match self.focus {
            FocusPane::Guilds => self.select_guild(),
            FocusPane::Channels => self.select_channel(),
            FocusPane::Messages => {
                self.jump_from_summary_todo();
                Vec::new()
            }
            FocusPane::Input => Vec::new(),
        }
    }

    /// If in summary view with a selected TODO, switch to messages view.
    fn jump_from_summary_todo(&mut self) {
        if self.message_pane_view != MessagePaneView::Summary {
            return;
        }
        // Switch back to messages view, preserving the summary for later
        self.message_pane_view = MessagePaneView::Messages;
    }

    fn select_guild(&mut self) -> Vec<Effect> {
        let Some(guild) = self.guild_state.selected().and_then(|i| self.guilds.get(i)) else {
            return Vec::new();
        };
        let guild_id = guild.id.clone();

        if let Some(g) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            g.unread = false;
            g.unread_count = 0;
        }

        self.selected_guild_id = Some(guild_id.clone());
        self.selected_channel_id = None;
        self.messages.clear();
        self.message_scroll_offset = 0;
        self.read_watermark = 0;
        self.unread_divider_anchor = None;
        self.unread_divider_message_id = None;
        self.message_pane_view = MessagePaneView::Messages;
        self.summary_state = SummaryPaneState::default();
        self.focus = FocusPane::Channels;

        // Use cached channels from GUILD_CREATE if available
        if let Some(channels) = self.guild_channels.get(&guild_id) {
            self.channels = channels.clone();
            self.channel_state = ListState::default();
            if let Some(idx) = self.first_selectable_channel_index() {
                self.channel_state.select(Some(idx));
            }
            Vec::new()
        } else {
            self.channels.clear();
            self.channel_state = ListState::default();
            vec![Effect::LoadChannels { guild_id }]
        }
    }

    fn select_channel(&mut self) -> Vec<Effect> {
        let Some(channel) = self
            .channel_state
            .selected()
            .and_then(|i| self.channels.get(i))
        else {
            return Vec::new();
        };
        if !channel.kind.is_selectable() {
            return Vec::new();
        }
        let channel_id = channel.id.clone();
        let had_unread = channel.unread;
        let stored_read = self
            .store
            .as_deref()
            .and_then(|store| store.last_read_message(&channel_id));
        tracing::debug!(
            channel_id = %channel_id,
            channel_name = %channel.name,
            kind = ?channel.kind,
            guild_id = ?channel.guild_id,
            had_unread,
            unread_count = channel.unread_count,
            last_message_id = ?channel.last_message_id,
            stored_read = ?stored_read,
            "opening channel"
        );

        // Clear unread in both the visible list and the per-guild cache
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel_id) {
            ch.unread = false;
            ch.unread_count = 0;
        }
        if let Some(guild_id) = &self.selected_guild_id
            && let Some(cached) = self.guild_channels.get_mut(guild_id)
            && let Some(ch) = cached.iter_mut().find(|c| c.id == channel_id)
        {
            ch.unread = false;
            ch.unread_count = 0;
        }
        if let Some(guild_id) = self.selected_guild_id.clone() {
            self.sync_guild_unread_from_channels(&guild_id);
        }

        self.selected_channel_id = Some(channel_id.clone());
        self.messages.clear();
        self.message_scroll_offset = 0;
        self.read_watermark = 0;
        self.unread_divider_message_id = None;
        self.unread_divider_anchor = if had_unread {
            self.store
                .as_deref()
                .and_then(|store| store.last_read_message(&channel_id))
                .map(UnreadDividerAnchor::AfterMessage)
                .or(Some(UnreadDividerAnchor::StartOfMessages))
        } else {
            None
        };
        self.message_pane_view = MessagePaneView::Messages;
        self.summary_state = SummaryPaneState::default();
        self.focus = FocusPane::Messages;

        // Persist session state
        if let Some(store) = &self.store {
            store.set_session(store::KEY_LAST_CHANNEL, &channel_id);
            if let Some(guild_id) = &self.selected_guild_id {
                store.set_session(store::KEY_LAST_GUILD, guild_id);
            }
        }

        vec![Effect::LoadHistory { channel_id }]
    }

    fn selectable_channel_indices(&self) -> Vec<usize> {
        self.channels
            .iter()
            .enumerate()
            .filter_map(|(idx, channel)| channel.kind.is_selectable().then_some(idx))
            .collect()
    }

    fn first_selectable_channel_index(&self) -> Option<usize> {
        self.channels
            .iter()
            .position(|channel| channel.kind.is_selectable())
    }

    fn move_channel_selection(&mut self, delta: i32) {
        let selectable = self.selectable_channel_indices();
        if selectable.is_empty() {
            return;
        }
        let current_raw = self.channel_state.selected().unwrap_or(selectable[0]);
        let current_pos = selectable
            .iter()
            .position(|idx| *idx == current_raw)
            .unwrap_or(0);
        let next_pos = if delta > 0 {
            current_pos.saturating_add(1).min(selectable.len() - 1)
        } else {
            current_pos.saturating_sub(1)
        };
        self.channel_state.select(Some(selectable[next_pos]));
    }

    fn sync_guild_list_offset(&mut self) {
        let Some(selected) = self.guild_state.selected() else {
            return;
        };
        // Keep the selected guild pinned in view when the guild rail collapses to avatars-only.
        *self.guild_state.offset_mut() = selected;
    }
}

fn move_list_selection(state: &mut ListState, len: usize, delta: i32) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let new = if delta > 0 {
        current.saturating_add(1).min(len - 1)
    } else {
        current.saturating_sub(1)
    };
    state.select(Some(new));
}

fn ghostty_progress_bar(tick_count: u32) -> String {
    const WIDTH: usize = 8;
    const SEGMENT_WIDTH: usize = 3;

    let mut cells = ['▱'; WIDTH];
    let offset = usize::try_from(tick_count).unwrap_or_default() % WIDTH;
    for segment_idx in 0..SEGMENT_WIDTH {
        let idx = (offset + segment_idx) % WIDTH;
        cells[idx] = '▰';
    }

    cells.iter().collect()
}

fn mark_channel_unread(channel: &mut ChannelSummary, message_id: &str) {
    channel.unread = true;
    channel.unread_count = channel.unread_count.saturating_add(1);
    channel.last_message_id = Some(message_id.to_string());
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use super::*;
    use crate::model::{ChannelKind, DIRECT_MESSAGES_GUILD_ID};
    use crate::transport::mock;

    /// Helper: create an app with mock guilds pre-loaded.
    fn app_with_guilds() -> App {
        let mut app = App::new();
        let effects = App::init_effects();
        assert!(effects.iter().any(|e| matches!(e, Effect::LoadGuilds)));
        app.update(Action::GuildsLoaded(mock::guilds()));
        app
    }

    /// Helper: navigate to first guild and first channel.
    fn app_in_channel() -> App {
        let mut app = app_with_guilds();
        app.focus = FocusPane::Guilds;
        let effects = app.update(Action::OpenSelected);
        // Execute the LoadChannels effect
        if let Some(Effect::LoadChannels { guild_id }) = effects.first() {
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild_id.clone()),
                channels: mock::channels(guild_id),
            });
        }
        app.focus = FocusPane::Channels;
        let effects = app.update(Action::OpenSelected);
        // Execute the LoadHistory effect
        if let Some(Effect::LoadHistory { channel_id }) = effects.first() {
            app.update(Action::HistoryLoaded {
                channel_id: channel_id.clone(),
                messages: mock::messages(channel_id),
                has_more: false,
            });
        }
        app
    }

    fn load_direct_messages(app: &mut App) -> String {
        let dm_index = app
            .guilds
            .iter()
            .position(|guild| guild.id == DIRECT_MESSAGES_GUILD_ID)
            .unwrap();
        app.guild_state.select(Some(dm_index));
        app.focus = FocusPane::Guilds;

        let effects = app.update(Action::OpenSelected);
        assert!(matches!(
            effects.first(),
            Some(Effect::LoadChannels { guild_id }) if guild_id == DIRECT_MESSAGES_GUILD_ID
        ));

        match &effects[0] {
            Effect::LoadChannels { guild_id } => guild_id.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn quit_sets_should_quit() {
        let mut app = App::new();
        assert!(!app.should_quit);
        app.update(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn focus_cycles_forward() {
        let mut app = App::new();
        assert_eq!(app.focus, FocusPane::Guilds);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, FocusPane::Channels);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, FocusPane::Messages);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, FocusPane::Input);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, FocusPane::Guilds);
    }

    #[test]
    fn focus_cycles_backward() {
        let mut app = App::new();
        app.update(Action::FocusPrev);
        assert_eq!(app.focus, FocusPane::Input);
        app.update(Action::FocusPrev);
        assert_eq!(app.focus, FocusPane::Messages);
    }

    #[test]
    fn enter_exit_insert_mode() {
        let mut app = App::new();
        app.update(Action::EnterInsert);
        assert_eq!(app.input_mode, InputMode::Insert);
        assert_eq!(app.focus, FocusPane::Input);
        app.update(Action::ExitInsert);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.focus, FocusPane::Messages);
    }

    #[test]
    fn toggle_help() {
        let mut app = App::new();
        assert!(!app.show_help);
        app.update(Action::ToggleHelp);
        assert!(app.show_help);
        app.update(Action::ToggleHelp);
        assert!(!app.show_help);
    }

    #[test]
    fn guild_selection_moves() {
        let mut app = app_with_guilds();
        app.guild_state.select(Some(0));
        app.focus = FocusPane::Guilds;

        app.update(Action::MoveDown);
        assert_eq!(app.guild_state.selected(), Some(1));
        app.update(Action::MoveDown);
        assert_eq!(app.guild_state.selected(), Some(2));
        app.update(Action::MoveDown);
        assert_eq!(app.guild_state.selected(), Some(3));
        // Clamp at end
        app.update(Action::MoveDown);
        assert_eq!(app.guild_state.selected(), Some(app.guilds.len() - 1));
        app.update(Action::MoveUp);
        assert_eq!(app.guild_state.selected(), Some(app.guilds.len() - 2));
    }

    #[test]
    fn message_scroll() {
        let mut app = App::new();
        app.focus = FocusPane::Messages;
        app.max_scroll = 100; // simulate enough content to scroll
        assert_eq!(app.message_scroll_offset, 0);
        app.update(Action::MoveDown);
        assert_eq!(app.message_scroll_offset, 1);
        app.update(Action::MoveUp);
        assert_eq!(app.message_scroll_offset, 0);
        app.update(Action::MoveUp);
        assert_eq!(app.message_scroll_offset, 0);
    }

    #[test]
    fn message_scroll_clamped_to_max() {
        let mut app = App::new();
        app.focus = FocusPane::Messages;
        app.max_scroll = 5;
        app.message_scroll_offset = 4;
        app.update(Action::MoveDown);
        assert_eq!(app.message_scroll_offset, 5);
        // Can't go past max
        app.update(Action::MoveDown);
        assert_eq!(app.message_scroll_offset, 5);
    }

    #[test]
    fn guilds_loaded_selects_first() {
        let mut app = App::new();
        app.update(Action::GuildsLoaded(vec![GuildSummary {
            id: "1".into(),
            name: "Test".into(),
            muted: false,
            unread: false,
            unread_count: 0,
            avatar_url: None,
        }]));
        assert_eq!(app.guild_state.selected(), Some(0));
        assert_eq!(app.guilds.len(), 2);
    }

    #[test]
    fn error_clears_after_tick() {
        let mut app = App::new();
        app.update(Action::Error("test error".into()));
        assert!(app.status_error().is_some());

        if let Some((_, ref mut at)) = app.status_error {
            *at = Instant::now()
                .checked_sub(std::time::Duration::from_secs(6))
                .unwrap();
        }

        app.update(Action::Tick);
        assert!(app.status_error().is_none());
    }

    #[test]
    fn message_jump_top_scrolls_to_oldest() {
        let mut app = App::new();
        app.focus = FocusPane::Messages;
        app.message_scroll_offset = 50;
        app.update(Action::JumpTop);
        assert_eq!(app.message_scroll_offset, 0);
    }

    #[test]
    fn message_jump_bottom_scrolls_to_newest() {
        let mut app = App::new();
        app.focus = FocusPane::Messages;
        app.at_bottom = false;
        app.update(Action::JumpBottom);
        assert!(app.at_bottom, "G should enable at_bottom auto-scroll");
    }

    #[test]
    fn help_popup_is_modal() {
        let mut app = app_with_guilds();
        app.focus = FocusPane::Guilds;

        app.update(Action::ToggleHelp);
        assert!(app.show_help);

        app.update(Action::FocusNext);
        assert_eq!(app.focus, FocusPane::Guilds);

        let selection_before = app.guild_state.selected();
        app.update(Action::MoveDown);
        assert_eq!(app.guild_state.selected(), selection_before);

        let mut app2 = App::new();
        app2.update(Action::ToggleHelp);
        app2.update(Action::Quit);
        assert!(app2.should_quit);

        app.update(Action::ToggleHelp);
        assert!(!app.show_help);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, FocusPane::Channels);
    }

    #[test]
    fn discord_token_prompt_is_modal_and_sets_disconnected_state() {
        let mut app = app_with_guilds();
        app.show_discord_token_prompt();

        assert!(app.has_discord_token_prompt());
        assert_eq!(app.connection_state, ConnectionState::Disconnected);

        let focus_before = app.focus;
        app.update(Action::FocusNext);
        assert_eq!(app.focus, focus_before);
    }

    #[test]
    fn discord_token_prompt_requires_non_empty_token() {
        let mut app = App::new();
        app.show_discord_token_prompt();

        assert!(app.take_discord_token_prompt_input().is_none());
        assert_eq!(
            app.discord_token_prompt()
                .and_then(|prompt| prompt.error.as_deref()),
            Some("Discord token is required")
        );

        app.push_discord_token_prompt_char(' ');
        app.push_discord_token_prompt_char('a');
        app.push_discord_token_prompt_char('b');
        app.push_discord_token_prompt_char('c');
        app.push_discord_token_prompt_char(' ');

        assert_eq!(
            app.take_discord_token_prompt_input().as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn ghostty_progress_bar_wraps_cleanly() {
        assert_eq!(ghostty_progress_bar(0), "▰▰▰▱▱▱▱▱");
        assert_eq!(ghostty_progress_bar(7), "▰▰▱▱▱▱▱▰");
        assert_eq!(ghostty_progress_bar(8), "▰▰▰▱▱▱▱▱");
    }

    #[test]
    fn set_focus_targets_specific_pane() {
        let mut app = App::new();
        app.update(Action::SetFocus(FocusPane::Messages));
        assert_eq!(app.focus, FocusPane::Messages);
        app.update(Action::SetFocus(FocusPane::Channels));
        assert_eq!(app.focus, FocusPane::Channels);
    }

    #[test]
    fn open_guild_emits_load_channels_effect() {
        let mut app = app_with_guilds();
        app.focus = FocusPane::Guilds;
        let effects = app.update(Action::OpenSelected);

        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Effect::LoadChannels { .. }));
        assert_eq!(app.focus, FocusPane::Channels);
        assert!(app.selected_guild_id.is_some());
    }

    #[test]
    fn guilds_loaded_adds_direct_messages_bucket() {
        let app = app_with_guilds();
        assert!(
            app.guilds
                .iter()
                .any(|guild| guild.id == DIRECT_MESSAGES_GUILD_ID)
        );
    }

    #[test]
    fn open_direct_messages_loads_private_channels() {
        let mut app = app_with_guilds();
        let guild_id = load_direct_messages(&mut app);
        assert_eq!(
            app.selected_guild_id.as_deref(),
            Some(DIRECT_MESSAGES_GUILD_ID)
        );
        assert_eq!(guild_id, DIRECT_MESSAGES_GUILD_ID);

        app.update(Action::ChannelsLoaded {
            guild_id: Some(guild_id.clone()),
            channels: mock::channels(&guild_id),
        });

        assert!(!app.channels.is_empty());
        assert!(
            app.channels
                .iter()
                .all(|channel| channel.kind == ChannelKind::DirectMessage)
        );
    }

    #[test]
    fn direct_messages_loaded_applies_local_read_state() {
        let mut app = app_with_guilds();
        app.store = Some(std::rc::Rc::new(Store::open_in_memory()));

        let guild_id = load_direct_messages(&mut app);
        let mut channels = mock::channels(&guild_id);
        let first_dm = channels
            .iter_mut()
            .find(|channel| channel.id == "dm1")
            .unwrap();
        first_dm.last_message_id = Some("100".into());

        app.store
            .as_ref()
            .unwrap()
            .mark_read(&first_dm.id, first_dm.last_message_id.as_deref().unwrap());

        app.update(Action::ChannelsLoaded {
            guild_id: Some(guild_id.clone()),
            channels,
        });

        let dm = app
            .channels
            .iter()
            .find(|channel| channel.id == "dm1")
            .unwrap();
        assert!(!dm.unread);
        assert_eq!(dm.unread_count, 0);
    }

    #[test]
    fn opening_unread_channel_sets_divider_at_first_unread_message() {
        let store = std::rc::Rc::new(Store::open_in_memory());
        store.mark_read("c1", "m3");

        let mut app = app_with_guilds();
        app.store = Some(store);

        app.focus = FocusPane::Guilds;
        let effects = app.update(Action::OpenSelected);
        if let Some(Effect::LoadChannels { guild_id }) = effects.first() {
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild_id.clone()),
                channels: mock::channels(guild_id),
            });
        }

        let unread_idx = app
            .channels
            .iter()
            .position(|channel| channel.id == "c1")
            .unwrap();
        app.channel_state.select(Some(unread_idx));
        app.focus = FocusPane::Channels;
        let effects = app.update(Action::OpenSelected);
        if let Some(Effect::LoadHistory { channel_id }) = effects.first() {
            app.update(Action::HistoryLoaded {
                channel_id: channel_id.clone(),
                messages: mock::messages(channel_id),
                has_more: false,
            });
        }

        assert_eq!(app.unread_divider_message_id(), Some("m4"));
        assert_eq!(app.read_watermark, 3);
    }

    #[test]
    fn load_priority_prefers_history_over_channel_list() {
        let mut app = App::new();
        app.selected_guild_id = Some("g1".into());
        app.selected_channel_id = Some("c1".into());
        app.start_load_scope(LoadScope::ChannelList("g1".into()));
        app.start_load_scope(LoadScope::History("c1".into()));

        assert_eq!(app.active_load_label(), Some("Refreshing history"));
        assert!(app.messages_pane_loading());
        assert!(!app.channels_pane_loading());
    }

    #[test]
    fn transport_connected_switches_to_guild_bootstrap_load() {
        let mut app = App::new();
        app.start_load_scope(LoadScope::StartupConnect);

        app.update(Action::TransportConnected {
            username: "you".into(),
        });

        assert_eq!(app.active_load_label(), Some("Loading guilds"));
        assert_eq!(app.connection_state, ConnectionState::Connected);
    }

    #[test]
    fn transport_connected_after_ready_data_does_not_restore_bootstrap_load() {
        let mut app = App::new();
        app.start_load_scope(LoadScope::StartupConnect);

        app.update(Action::ReadyData {
            guilds: mock::guilds(),
            guild_channels: HashMap::new(),
            guild_mute_settings: HashMap::new(),
        });
        app.update(Action::TransportConnected {
            username: "you".into(),
        });

        assert_eq!(app.active_load_label(), None);
        assert_eq!(app.connection_state, ConnectionState::Connected);
    }

    #[test]
    fn stale_channels_loaded_clears_matching_load_scope() {
        let mut app = App::new();
        app.selected_guild_id = Some("g2".into());
        app.start_load_scope(LoadScope::ChannelList("g1".into()));

        app.update(Action::ChannelsLoaded {
            guild_id: Some("g1".into()),
            channels: Vec::new(),
        });

        assert_eq!(app.active_load_label(), None);
    }

    #[test]
    fn load_failed_clears_scope_and_sets_error() {
        let mut app = App::new();
        app.start_load_scope(LoadScope::History("c1".into()));

        app.update(Action::LoadFailed {
            scope: LoadScope::History("c1".into()),
            message: "boom".into(),
        });

        assert_eq!(app.active_load_label(), None);
        assert_eq!(app.status_error(), Some("boom"));
    }

    #[test]
    fn muted_guild_hides_guild_unread_but_not_channel_rows() {
        let mut app = app_with_guilds();
        app.focus = FocusPane::Guilds;
        let effects = app.update(Action::OpenSelected);
        if let Some(Effect::LoadChannels { guild_id }) = effects.first() {
            let mut channels = mock::channels(guild_id);
            for channel in &mut channels {
                channel.unread = false;
                channel.unread_count = 0;
            }
            if let Some(channel) = channels
                .iter_mut()
                .find(|channel| channel.kind.is_selectable())
            {
                channel.unread = true;
                channel.unread_count = 7;
            }
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild_id.clone()),
                channels,
            });
        }

        app.update(Action::GuildMuteSettingsUpdated(GuildMuteSettings {
            guild_id: "g1".into(),
            muted: true,
            channel_overrides: HashMap::new(),
        }));

        assert!(
            app.guilds
                .iter()
                .find(|guild| guild.id == "g1")
                .unwrap()
                .muted
        );
        assert_eq!(
            app.guilds
                .iter()
                .find(|guild| guild.id == "g1")
                .unwrap()
                .unread_count,
            0
        );
        assert!(
            app.channels
                .iter()
                .filter(|channel| channel.kind.is_selectable())
                .any(ChannelSummary::shows_unread_in_channel_list)
        );
        assert_eq!(
            app.channels
                .iter()
                .find(|channel| channel.kind.is_selectable())
                .unwrap()
                .unread_count,
            7
        );
    }

    #[test]
    fn open_channel_emits_load_history_effect() {
        let app = app_in_channel();
        // app_in_channel already opened a channel, verify state
        assert!(!app.messages.is_empty());
        assert_eq!(app.focus, FocusPane::Messages);
        assert!(app.selected_channel_id.is_some());
    }

    #[test]
    fn manual_refresh_emits_channel_and_history_effects() {
        let mut app = app_in_channel();

        let effects = app.update(Action::RefreshNow);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadChannels { .. }))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadHistory { .. }))
        );
    }

    #[test]
    fn tick_does_not_auto_refresh_live_transport() {
        let mut app = app_in_channel();
        app.connection_state = ConnectionState::Connected;
        app.tick_count = 39;

        let effects = app.update(Action::Tick);

        assert!(effects.is_empty());
    }

    #[test]
    fn open_guild_clears_unread() {
        let mut app = app_with_guilds();
        assert!(app.guilds[0].unread);
        app.focus = FocusPane::Guilds;
        app.update(Action::OpenSelected);
        assert!(!app.guilds[0].unread);
    }

    #[test]
    fn open_channel_clears_unread() {
        let mut app = app_with_guilds();
        app.focus = FocusPane::Guilds;
        let effects = app.update(Action::OpenSelected);
        if let Some(Effect::LoadChannels { guild_id }) = effects.first() {
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild_id.clone()),
                channels: mock::channels(guild_id),
            });
        }

        let unread_idx = app.channels.iter().position(|ch| ch.unread);
        if let Some(idx) = unread_idx {
            app.focus = FocusPane::Channels;
            app.channel_state.select(Some(idx));
            app.update(Action::OpenSelected);
            assert!(!app.channels[idx].unread);
        }
    }

    #[test]
    fn send_appends_message_and_emits_effect() {
        let mut app = app_in_channel();
        let msg_count = app.messages.len();
        app.input_text = "Hello world!".into();
        let effects = app.update(Action::SendCurrentMessage);

        assert_eq!(app.messages.len(), msg_count + 1);
        assert_eq!(app.messages.last().unwrap().author, "you");
        assert_eq!(app.messages.last().unwrap().content, "Hello world!");
        assert!(app.input_text.is_empty());
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Effect::SendMessage { .. }));
    }

    #[test]
    fn send_empty_does_nothing() {
        let mut app = app_in_channel();
        let msg_count = app.messages.len();
        app.input_text = "   ".into();
        let effects = app.update(Action::SendCurrentMessage);
        assert_eq!(app.messages.len(), msg_count);
        assert!(effects.is_empty());
    }

    #[test]
    fn send_without_channel_preserves_draft() {
        let mut app = App::new();
        app.selected_channel_id = None;
        app.input_text = "my draft".into();
        let effects = app.update(Action::SendCurrentMessage);
        assert_eq!(app.input_text, "my draft");
        assert!(app.status_error().is_some());
        assert!(effects.is_empty());
    }

    #[test]
    fn consecutive_sends_get_continuation() {
        let mut app = app_in_channel();

        app.input_text = "First message".into();
        app.update(Action::SendCurrentMessage);
        app.input_text = "Second message".into();
        app.input_mode = InputMode::Insert;
        app.update(Action::SendCurrentMessage);

        let last = app.messages.last().unwrap();
        assert!(last.is_continuation);
    }

    #[test]
    fn switch_guild_resets_summary_state() {
        let mut app = app_in_channel();

        app.update(Action::RequestSummary);
        assert_eq!(app.message_pane_view, MessagePaneView::Summary);
        assert!(app.summary_state.in_flight);

        app.focus = FocusPane::Guilds;
        app.guild_state.select(Some(1));
        app.update(Action::OpenSelected);

        assert_eq!(app.message_pane_view, MessagePaneView::Messages);
        assert!(!app.summary_state.in_flight);
    }

    #[test]
    fn send_preserves_multiline_content() {
        let mut app = app_in_channel();
        app.input_text = "line one\nline two\n".into();
        app.update(Action::SendCurrentMessage);
        assert_eq!(app.messages.last().unwrap().content, "line one\nline two\n");
    }

    #[test]
    fn send_preserves_leading_whitespace() {
        let mut app = app_in_channel();
        app.input_text = "  indented text  ".into();
        app.update(Action::SendCurrentMessage);
        assert_eq!(app.messages.last().unwrap().content, "  indented text  ");
    }

    #[test]
    fn request_summary_emits_effect() {
        let mut app = app_in_channel();
        let effects = app.update(Action::RequestSummary);

        assert!(app.summary_state.in_flight);
        assert_eq!(app.message_pane_view, MessagePaneView::Summary);
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Effect::SummarizeChannel { .. }));
    }

    #[test]
    fn message_append_at_bottom_stays_at_bottom() {
        let mut app = app_in_channel();
        // At bottom (offset 0)
        assert_eq!(app.message_scroll_offset, 0);

        app.update(Action::MessageAppended {
            message: MessageRow {
                id: "new1".into(),
                channel_id: app.selected_channel_id.clone().unwrap(),
                author: "someone".into(),
                author_avatar_url: None,
                content: "new message".into(),
                attachments: Vec::new(),
                timestamp: "12:00".into(),
                edited: false,
                is_continuation: false,
            },
            channel_hint: None,
        });

        assert_eq!(
            app.message_scroll_offset, 0,
            "should stay at bottom when new message arrives"
        );
    }

    #[test]
    fn message_append_while_scrolled_preserves_position() {
        let mut app = app_in_channel();
        app.focus = FocusPane::Messages;
        app.max_scroll = 100;
        app.update(Action::MoveDown);
        app.update(Action::MoveDown);
        assert_eq!(app.message_scroll_offset, 2);

        app.update(Action::MessageAppended {
            message: MessageRow {
                id: "new2".into(),
                channel_id: app.selected_channel_id.clone().unwrap(),
                author: "someone".into(),
                author_avatar_url: None,
                content: "new while scrolled".into(),
                attachments: Vec::new(),
                timestamp: "12:01".into(),
                edited: false,
                is_continuation: false,
            },
            channel_hint: None,
        });

        assert_eq!(
            app.message_scroll_offset, 2,
            "scroll position preserved when new message arrives while scrolled up"
        );
    }

    #[test]
    fn first_history_load_scrolls_to_bottom() {
        let mut app = app_in_channel();
        app.focus = FocusPane::Messages;
        // Simulate switching to a new channel (no messages yet)
        app.messages.clear();
        app.at_bottom = false;

        app.update(Action::HistoryLoaded {
            channel_id: app.selected_channel_id.clone().unwrap(),
            messages: mock::messages("c1"),
            has_more: false,
        });

        assert!(
            app.at_bottom,
            "first history load should auto-scroll to bottom"
        );
    }

    #[test]
    fn refresh_preserves_scroll_position() {
        let mut app = app_in_channel();
        app.focus = FocusPane::Messages;
        // User scrolled up
        app.at_bottom = false;
        app.message_scroll_offset = 5;

        // Refresh with same messages
        app.update(Action::HistoryLoaded {
            channel_id: app.selected_channel_id.clone().unwrap(),
            messages: mock::messages("c1"),
            has_more: false,
        });

        assert!(
            !app.at_bottom,
            "refresh should preserve scroll position when scrolled up"
        );
    }

    #[test]
    fn history_load_for_wrong_channel_ignored() {
        let mut app = app_in_channel();
        let original_count = app.messages.len();
        app.message_scroll_offset = 5;

        app.update(Action::HistoryLoaded {
            channel_id: "wrong_channel".into(),
            messages: vec![],
            has_more: false,
        });

        assert_eq!(
            app.messages.len(),
            original_count,
            "messages unchanged for wrong channel"
        );
        assert_eq!(
            app.message_scroll_offset, 5,
            "scroll unchanged for wrong channel"
        );
    }

    #[test]
    fn stale_channels_loaded_rejected() {
        let mut app = app_with_guilds();
        // Select first guild
        app.focus = FocusPane::Guilds;
        app.guild_state.select(Some(0));
        app.update(Action::OpenSelected);
        let first_guild_id = app.selected_guild_id.clone().unwrap();

        // Switch to second guild before first response arrives
        app.focus = FocusPane::Guilds;
        app.guild_state.select(Some(1));
        app.update(Action::OpenSelected);
        let second_guild_id = app.selected_guild_id.clone().unwrap();
        assert_ne!(first_guild_id, second_guild_id);

        // Stale response for first guild arrives
        app.update(Action::ChannelsLoaded {
            guild_id: Some(first_guild_id),
            channels: mock::channels("g1"),
        });

        // Channels should still be empty (waiting for second guild's response)
        assert!(
            app.channels.is_empty(),
            "stale channel response should be rejected"
        );

        // Correct response for second guild arrives
        app.update(Action::ChannelsLoaded {
            guild_id: Some(second_guild_id),
            channels: mock::channels("g2"),
        });

        assert!(
            !app.channels.is_empty(),
            "correct channel response should be accepted"
        );
    }

    #[test]
    fn request_summary_without_channel_shows_error() {
        let mut app = App::new();
        app.update(Action::GuildsLoaded(mock::guilds()));
        // No channel selected
        assert!(app.selected_channel_id.is_none());

        let effects = app.update(Action::RequestSummary);

        assert!(effects.is_empty(), "no effect emitted without channel");
        assert!(
            !app.summary_state.in_flight,
            "should not enter in_flight without channel"
        );
        assert_eq!(
            app.message_pane_view,
            MessagePaneView::Messages,
            "should not switch to summary view"
        );
        assert!(app.status_error().is_some(), "should show error");
    }

    #[test]
    fn esc_in_normal_mode_exits_summary_view() {
        let mut app = app_in_channel();
        // Enter summary view
        app.update(Action::ShowSummary);
        assert_eq!(app.message_pane_view, MessagePaneView::Summary);

        // ShowMessages (mapped from Esc in normal mode) should exit
        app.update(Action::ShowMessages);
        assert_eq!(app.message_pane_view, MessagePaneView::Messages);
    }

    #[test]
    fn stale_summary_result_rejected() {
        let mut app = app_in_channel();
        let original_channel = app.selected_channel_id.clone().unwrap();

        // Request summary
        app.update(Action::RequestSummary);
        assert!(app.summary_state.in_flight);

        // Switch to a different channel before summary arrives
        app.focus = FocusPane::Guilds;
        app.guild_state.select(Some(1));
        app.update(Action::OpenSelected);
        // Load channels for new guild
        let effects = app.update(Action::OpenSelected);
        if let Some(Effect::LoadChannels { guild_id }) = effects.first() {
            app.update(Action::ChannelsLoaded {
                guild_id: Some(guild_id.clone()),
                channels: mock::channels(guild_id),
            });
        }

        // Stale summary arrives for the original channel
        app.update(Action::SummaryReady(ChannelDigest {
            channel_id: original_channel,
            summary: "stale summary".into(),
            todos: vec![],
            generated_at: "00:00".into(),
        }));

        assert!(
            app.summary_state.last_digest.is_none(),
            "stale summary should be rejected"
        );
    }

    #[test]
    fn summary_sends_unread_messages_only() {
        let mut app = app_in_channel();
        let msg_count = app.messages.len();
        // read_watermark should be set to initial message count
        assert_eq!(app.read_watermark, msg_count);

        // Simulate new messages arriving (unread)
        let ch_id = app.selected_channel_id.clone().unwrap();
        app.update(Action::MessageAppended {
            message: MessageRow {
                id: "new1".into(),
                channel_id: ch_id.clone(),
                author: "someone".into(),
                author_avatar_url: None,
                content: "unread message".into(),
                attachments: Vec::new(),
                timestamp: "15:00".into(),
                edited: false,
                is_continuation: false,
            },
            channel_hint: None,
        });

        let effects = app.update(Action::RequestSummary);
        assert_eq!(effects.len(), 1);
        assert!(
            matches!(&effects[0], Effect::SummarizeChannel { messages, .. } if messages.len() == 1),
            "should emit SummarizeChannel with exactly 1 unread message"
        );
    }

    #[test]
    fn background_message_with_channel_hint_marks_unknown_channel_unread() {
        let mut app = app_in_channel();
        let guild_id = app.selected_guild_id.clone().unwrap();
        let hinted_channel_id = "c-new".to_string();

        app.update(Action::MessageAppended {
            message: MessageRow {
                id: "m-new".into(),
                channel_id: hinted_channel_id.clone(),
                author: "someone".into(),
                author_avatar_url: None,
                content: "background update".into(),
                attachments: Vec::new(),
                timestamp: "16:00".into(),
                edited: false,
                is_continuation: false,
            },
            channel_hint: Some(ChannelSummary {
                id: hinted_channel_id.clone(),
                guild_id: Some(guild_id.clone()),
                parent_id: None,
                name: "new-channel".into(),
                kind: ChannelKind::Text,
                position: 999,
                muted: false,
                unread: false,
                unread_count: 0,
                last_message_id: None,
            }),
        });

        let visible = app
            .channels
            .iter()
            .find(|channel| channel.id == hinted_channel_id)
            .unwrap();
        assert!(visible.unread);
        assert_eq!(visible.unread_count, 1);

        let cached = app
            .guild_channels
            .get(&guild_id)
            .unwrap()
            .iter()
            .find(|channel| channel.id == hinted_channel_id)
            .unwrap();
        assert!(cached.unread);
        assert_eq!(cached.unread_count, 1);

        let guild = app
            .guilds
            .iter()
            .find(|guild| guild.id == guild_id)
            .unwrap();
        assert!(guild.unread);
        assert!(guild.unread_count >= 1);
    }

    #[test]
    fn refresh_detects_new_background_messages_even_for_muted_guilds() {
        let mut app = app_in_channel();
        let guild_id = app.selected_guild_id.clone().unwrap();
        let selected_channel_id = app.selected_channel_id.clone().unwrap();

        if let Some(cached_channels) = app.guild_channels.get_mut(&guild_id) {
            for channel in cached_channels {
                channel.unread = false;
                channel.unread_count = 0;
                channel.last_message_id = Some("100".into());
            }
        }
        for channel in &mut app.channels {
            channel.unread = false;
            channel.unread_count = 0;
            channel.last_message_id = Some("100".into());
        }
        if let Some(guild) = app.guilds.iter_mut().find(|guild| guild.id == guild_id) {
            guild.unread = false;
            guild.unread_count = 0;
        }

        app.update(Action::GuildMuteSettingsUpdated(GuildMuteSettings {
            guild_id: guild_id.clone(),
            muted: true,
            channel_overrides: HashMap::new(),
        }));

        let mut refreshed = mock::channels(&guild_id);
        for channel in &mut refreshed {
            channel.unread = false;
            channel.unread_count = 0;
            channel.last_message_id = Some("100".into());
        }
        let background_id = refreshed
            .iter_mut()
            .find(|channel| channel.kind.is_selectable() && channel.id != selected_channel_id)
            .map(|channel| {
                channel.last_message_id = Some("200".into());
                channel.id.clone()
            })
            .unwrap();

        app.update(Action::ChannelsLoaded {
            guild_id: Some(guild_id.clone()),
            channels: refreshed,
        });

        let visible = app
            .channels
            .iter()
            .find(|channel| channel.id == background_id)
            .unwrap();
        assert!(visible.unread);
        assert_eq!(visible.unread_count, 1);

        let guild = app
            .guilds
            .iter()
            .find(|guild| guild.id == guild_id)
            .unwrap();
        assert!(!guild.unread);
        assert_eq!(guild.unread_count, 0);
    }

    #[test]
    fn hinted_muted_channel_stays_hidden_in_channel_list() {
        let mut app = app_in_channel();
        let guild_id = app.selected_guild_id.clone().unwrap();
        let hinted_channel_id = "c-muted".to_string();

        if let Some(cached_channels) = app.guild_channels.get_mut(&guild_id) {
            for channel in cached_channels {
                channel.unread = false;
                channel.unread_count = 0;
            }
        }
        for channel in &mut app.channels {
            channel.unread = false;
            channel.unread_count = 0;
        }
        if let Some(guild) = app.guilds.iter_mut().find(|guild| guild.id == guild_id) {
            guild.unread = false;
            guild.unread_count = 0;
        }

        app.update(Action::GuildMuteSettingsUpdated(GuildMuteSettings {
            guild_id: guild_id.clone(),
            muted: false,
            channel_overrides: HashMap::from([(
                hinted_channel_id.clone(),
                crate::model::ChannelMuteOverride { muted: true },
            )]),
        }));

        app.update(Action::MessageAppended {
            message: MessageRow {
                id: "m-muted".into(),
                channel_id: hinted_channel_id.clone(),
                author: "someone".into(),
                author_avatar_url: None,
                content: "background update".into(),
                attachments: Vec::new(),
                timestamp: "16:05".into(),
                edited: false,
                is_continuation: false,
            },
            channel_hint: Some(ChannelSummary {
                id: hinted_channel_id.clone(),
                guild_id: Some(guild_id.clone()),
                parent_id: None,
                name: "muted-thread".into(),
                kind: ChannelKind::Text,
                position: 1000,
                muted: false,
                unread: false,
                unread_count: 0,
                last_message_id: None,
            }),
        });

        let visible = app
            .channels
            .iter()
            .find(|channel| channel.id == hinted_channel_id)
            .unwrap();
        assert!(visible.muted);
        assert!(!visible.shows_unread_in_channel_list());

        let guild = app
            .guilds
            .iter()
            .find(|guild| guild.id == guild_id)
            .unwrap();
        assert!(!guild.unread);
        assert_eq!(guild.unread_count, 0);
    }

    #[test]
    fn esc_exits_summary_view_in_normal_mode() {
        let mut app = app_in_channel();
        app.update(Action::ShowSummary);
        assert_eq!(app.message_pane_view, MessagePaneView::Summary);

        // ShowMessages is what Esc maps to in normal mode (event.rs line 57)
        app.update(Action::ShowMessages);
        assert_eq!(
            app.message_pane_view,
            MessagePaneView::Messages,
            "Esc/m should exit summary view"
        );
    }
}
