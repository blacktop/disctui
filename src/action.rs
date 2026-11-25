use std::collections::HashMap;
use std::time::Instant;

use crate::app::FocusPane;
use crate::digest::DigestReport;
use crate::model::{
    ChannelDigest, ChannelSummary, GuildMuteSettings, GuildSummary, LoadScope, MessageRow,
};

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
    /// Open a channel by id, from `--open` or the control socket.
    OpenChannel(String),
    CloseSelectedDirectMessage,
    RefreshNow,
    ReconnectTransport,
    SelectMessage {
        older: bool,
    },
    CopyMessage,
    OpenMessageLink,
    DesktopFinished(Result<&'static str, &'static str>),
    LoadStarted(LoadScope),
    LoadFailed {
        scope: LoadScope,
        message: String,
    },
    EnterInsert,
    ExitInsert,
    ToggleHelp,
    SubmitDiscordToken,
    /// Leave the sign-in prompt and browse sample data.
    CancelDiscordToken,
    /// Switch the sign-in prompt to pasting a token.
    ShowTokenEntry,
    /// Leave the token field for the first sign-in screen.
    LeaveTokenEntry,
    /// Open Discord's own sign-in window for a message session.
    #[cfg(feature = "experimental-discord")]
    WebSignIn,
    /// How far that window has got.
    #[cfg(feature = "experimental-discord")]
    WebSignInProgress(crate::app::WebStage),
    /// The window saved a validated session in the Keychain.
    #[cfg(feature = "experimental-discord")]
    WebSignInSaved,
    /// Why the window produced no session.
    #[cfg(feature = "experimental-discord")]
    WebSignInFailed(String),
    #[cfg(feature = "experimental-discord")]
    BrowserSignIn,
    #[cfg(feature = "experimental-discord")]
    BrowserSignOut,
    #[cfg(feature = "experimental-discord")]
    CancelBrowser,

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
    TransportSessionExpired,
    #[cfg(feature = "experimental-discord")]
    MessageSessionRejected {
        session: std::time::Instant,
        channel_id: String,
        message: String,
    },

    // Data loading
    GuildsLoaded(Vec<GuildSummary>),
    ReadyData {
        guilds: Vec<GuildSummary>,
        guild_channels: HashMap<String, Vec<ChannelSummary>>,
        guild_mute_settings: HashMap<String, GuildMuteSettings>,
    },
    GuildAvailable {
        guild: GuildSummary,
        channels: Vec<ChannelSummary>,
    },
    GuildMuteSettingsUpdated(GuildMuteSettings),
    ChannelsLoaded {
        guild_id: Option<String>,
        channels: Vec<ChannelSummary>,
    },
    HistoryLoaded {
        channel_id: String,
        messages: Vec<MessageRow>,
        has_more: bool,
    },
    MessageAppended {
        message: MessageRow,
        channel_hint: Option<ChannelSummary>,
    },
    MessagePatched(MessageRow),
    MessageRemoved {
        channel_id: String,
        message_id: String,
    },

    // AI
    SummaryReady(ChannelDigest),
    SummaryFailed(String),
    RegenerateDigest,
    DigestReady {
        job: Instant,
        report: DigestReport,
    },
    AvatarLoaded {
        url: String,
        bytes: Vec<u8>,
    },
    AvatarFailed(String),

    Error(String),
}

/// Where an action comes from. A modal popup drops what the person typed and
/// keeps what the transport, the AI backend, and the control socket deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A key press or mouse click.
    Input,
    /// The tick timer or a terminal resize.
    Clock,
    /// Anything else: transport events, finished background work, control requests.
    Delivered,
}

impl Action {
    pub const fn origin(&self) -> Origin {
        match self {
            Self::Tick | Self::Resize { .. } => Origin::Clock,
            Self::Quit
            | Self::FocusNext
            | Self::FocusPrev
            | Self::SetFocus(_)
            | Self::MoveUp
            | Self::MoveDown
            | Self::JumpTop
            | Self::JumpBottom
            | Self::ScrollUp(_)
            | Self::ScrollDown(_)
            | Self::OpenSelected
            | Self::CloseSelectedDirectMessage
            | Self::RefreshNow
            | Self::ReconnectTransport
            | Self::SelectMessage { .. }
            | Self::CopyMessage
            | Self::OpenMessageLink
            | Self::EnterInsert
            | Self::ExitInsert
            | Self::ToggleHelp
            | Self::SubmitDiscordToken
            | Self::CancelDiscordToken
            | Self::ShowTokenEntry
            | Self::LeaveTokenEntry
            | Self::MarkAllRead
            | Self::ShowMessages
            | Self::ShowSummary
            | Self::RequestSummary
            | Self::SummarySelectNextTodo
            | Self::SummarySelectPrevTodo
            | Self::JumpToTodoMessage
            | Self::StartQuickReplyFromSummary
            | Self::SendCurrentMessage
            | Self::RegenerateDigest => Origin::Input,
            #[cfg(feature = "experimental-discord")]
            Self::WebSignIn | Self::BrowserSignIn | Self::BrowserSignOut | Self::CancelBrowser => {
                Origin::Input
            }
            Self::OpenChannel(_)
            | Self::LoadStarted(_)
            | Self::LoadFailed { .. }
            | Self::TransportConnecting
            | Self::TransportConnected { .. }
            | Self::TransportDisconnected(_)
            | Self::TransportSessionExpired
            | Self::GuildsLoaded(_)
            | Self::ReadyData { .. }
            | Self::GuildAvailable { .. }
            | Self::GuildMuteSettingsUpdated(_)
            | Self::ChannelsLoaded { .. }
            | Self::HistoryLoaded { .. }
            | Self::MessageAppended { .. }
            | Self::MessagePatched(_)
            | Self::MessageRemoved { .. }
            | Self::SummaryReady(_)
            | Self::SummaryFailed(_)
            | Self::DigestReady { .. }
            | Self::AvatarLoaded { .. }
            | Self::AvatarFailed(_)
            | Self::DesktopFinished(_)
            | Self::Error(_) => Origin::Delivered,
            #[cfg(feature = "experimental-discord")]
            Self::WebSignInProgress(_)
            | Self::WebSignInSaved
            | Self::WebSignInFailed(_)
            | Self::MessageSessionRejected { .. } => Origin::Delivered,
        }
    }
}
