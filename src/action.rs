use std::collections::HashMap;

use crate::app::FocusPane;
use crate::model::{ChannelDigest, ChannelSummary, GuildSummary, MessageRow};

#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "variants constructed by transport bridge and matched in app.update()"
)]
pub enum Action {
    Tick,
    Quit,
    Resize {
        width: u16,
        height: u16,
    },

    // Navigation
    FocusNext,
    FocusPrev,
    SetFocus(FocusPane),
    MoveUp,
    MoveDown,
    JumpTop,
    JumpBottom,
    ScrollUp(u16),
    ScrollDown(u16),
    OpenSelected,
    EnterInsert,
    ExitInsert,
    ToggleHelp,

    // Read state
    MarkAllRead,

    // Message pane views
    ShowMessages,
    ShowSummary,
    RequestSummary,
    SummarySelectNextTodo,
    SummarySelectPrevTodo,
    JumpToTodoMessage,
    StartQuickReplyFromSummary,

    // Composer
    SendCurrentMessage,

    // Transport state
    TransportConnecting,
    TransportConnected {
        username: String,
    },
    TransportDisconnected(String),

    // Data loading
    GuildsLoaded(Vec<GuildSummary>),
    ReadyData {
        guilds: Vec<GuildSummary>,
        guild_channels: HashMap<String, Vec<ChannelSummary>>,
    },
    GuildAvailable {
        guild: GuildSummary,
        channels: Vec<ChannelSummary>,
    },
    ChannelsLoaded {
        guild_id: Option<String>,
        channels: Vec<ChannelSummary>,
    },
    HistoryLoaded {
        channel_id: String,
        messages: Vec<MessageRow>,
        has_more: bool,
    },
    MessageAppended(MessageRow),
    MessagePatched(MessageRow),
    MessageRemoved {
        channel_id: String,
        message_id: String,
    },

    // AI
    SummaryReady(ChannelDigest),
    SummaryFailed(String),
    AvatarLoaded {
        url: String,
        bytes: Vec<u8>,
    },
    AvatarFailed(String),

    Error(String),
}
