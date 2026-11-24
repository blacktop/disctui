//! Daily digest: pick the active channels across servers, then shape fetched
//! messages and AI summaries into a report that renders as text, Markdown, or JSON.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use color_eyre::eyre::Result;
#[cfg(feature = "experimental-discord")]
use color_eyre::eyre::{WrapErr, eyre};

use serde::Serialize;
use unicode_width::UnicodeWidthStr;

use crate::ai::SummarizerBackend;
use crate::transport::mock;

use crate::model::{ChannelDigest, ChannelKind, ChannelSummary, GuildSummary, MessageRow};

const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
const SNIPPET_MAX_CHARS: usize = 120;
pub const DEFAULT_WINDOW: Duration = Duration::from_hours(24);
pub const DEFAULT_TOP: usize = 3;
pub const DEFAULT_LIMIT: usize = 15;

/// Creation time encoded in a Discord snowflake id, in Unix milliseconds.
pub fn snowflake_timestamp_ms(id: &str) -> Option<u64> {
    let value = id.parse::<u64>().ok()?;
    (value >> 22).checked_add(DISCORD_EPOCH_MS)
}

/// Parse a look-back window such as `24h`, `90m`, `2d`, or `1w`.
pub fn parse_window(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let invalid = || format!("invalid window {text:?}; use a number plus m, h, d, or w (e.g. 24h)");

    let unit = text.chars().last().ok_or_else(invalid)?;
    let digits = text.trim_end_matches(unit);
    let count: u64 = digits.parse().map_err(|_| invalid())?;
    let unit_secs: u64 = match unit {
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        'w' => 604_800,
        _ => return Err(invalid()),
    };
    if count == 0 {
        return Err(invalid());
    }
    count
        .checked_mul(unit_secs)
        .map(Duration::from_secs)
        .ok_or_else(invalid)
}

/// Human label for a window, e.g. `last 24h`.
pub fn window_label(window: Duration) -> String {
    let secs = window.as_secs();
    let hours = secs / 3_600;
    if secs.is_multiple_of(86_400) && hours >= 48 {
        format!("last {}d", secs / 86_400)
    } else if secs.is_multiple_of(3_600) {
        format!("last {hours}h")
    } else {
        format!("last {}m", secs / 60)
    }
}

#[derive(Debug, Clone)]
pub struct DigestOptions {
    pub window: Duration,
    /// Server names (case-insensitive substring) or ids. Empty means every server.
    pub guild_filters: Vec<String>,
    /// The whitelist. Empty means every server that passes `guild_filters`.
    pub include: Vec<Include>,
    pub top_per_server: usize,
    pub max_channels: usize,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            window: DEFAULT_WINDOW,
            guild_filters: Vec::new(),
            include: Vec::new(),
            top_per_server: DEFAULT_TOP,
            max_channels: DEFAULT_LIMIT,
        }
    }
}

/// One whitelist entry: a whole server, or one channel of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    server: String,
    channel: Option<String>,
}

impl Include {
    /// Parse `Server` or `Server #channel`. A name matches as a case-insensitive
    /// substring and an id matches exactly. `#channel` alone means any server.
    pub fn parse(entry: &str) -> Option<Self> {
        let (server, channel) = match entry.split_once('#') {
            Some((server, channel)) => (server, Some(channel)),
            None => (entry, None),
        };
        let server = server.trim().to_lowercase();
        let channel = channel
            .map(|channel| channel.trim().to_lowercase())
            .filter(|channel| !channel.is_empty());
        if server.is_empty() && channel.is_none() {
            return None;
        }
        Some(Self { server, channel })
    }

    fn names_guild(&self, guild: &GuildSummary) -> bool {
        self.server.is_empty()
            || guild.id == self.server
            || guild.name.to_lowercase().contains(&self.server)
    }
}

/// What the whitelist admits from one server.
struct Scope<'a> {
    whole_server: bool,
    channels: Vec<&'a str>,
}

impl<'a> Scope<'a> {
    fn of(guild: &GuildSummary, include: &'a [Include]) -> Self {
        let mut scope = Self {
            whole_server: include.is_empty(),
            channels: Vec::new(),
        };
        for entry in include.iter().filter(|entry| entry.names_guild(guild)) {
            match &entry.channel {
                Some(channel) => scope.channels.push(channel),
                None => scope.whole_server = true,
            }
        }
        scope
    }

    fn admits_nothing(&self) -> bool {
        !self.whole_server && self.channels.is_empty()
    }

    fn names(&self, channel: &ChannelSummary) -> bool {
        let name = channel.name.to_lowercase();
        self.channels
            .iter()
            .any(|needle| channel.id == *needle || name.contains(needle))
    }
}

/// A digest captures the channel selection and time window before fetching history.
#[derive(Debug, Clone)]
pub struct PreparedDigest {
    selection: Selection,
    window: Duration,
    cutoff_ms: u64,
    user_name: String,
}

impl PreparedDigest {
    pub fn new(
        guilds: &[GuildSummary],
        channels: &HashMap<String, Vec<ChannelSummary>>,
        options: &DigestOptions,
        user_name: String,
    ) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        Self {
            selection: select_candidates(guilds, channels, options, now_ms),
            window: options.window,
            cutoff_ms: window_cutoff(options.window, now_ms),
            user_name,
        }
    }

    pub async fn generate(
        self,
        source: HistorySource,
        backend: Option<&SummarizerBackend>,
        mut on_read: impl FnMut(usize, usize, &str),
    ) -> DigestReport {
        let total = self.selection.candidates.len();
        let mut activities = Vec::with_capacity(total);
        for (index, candidate) in self.selection.candidates.into_iter().enumerate() {
            let channel = &candidate.channel;
            let label = format!(
                "{} {}{}",
                candidate.guild_name,
                channel.kind.marker(),
                channel.name
            );
            on_read(index, total, &label);
            let (history, error) = match source.history(&channel.id, self.cutoff_ms).await {
                Ok(mut history) => {
                    let note = history.limit_note.take();
                    (history, note)
                }
                Err(err) => {
                    tracing::warn!(
                        "digest: could not read {}: {}",
                        terminal_field(&label),
                        terminal_field(&err.to_string())
                    );
                    (History::default(), Some("Could not load history".into()))
                }
            };

            let mut activity = ChannelActivity {
                candidate,
                messages: history.messages,
                mention_ids: history.mention_ids,
                digest: None,
                error,
            };
            if let Some(backend) = backend
                && activity.error.is_none()
                && !activity.messages.is_empty()
            {
                let name = &activity.candidate.channel.name;
                match backend
                    .summarize(name, &activity.messages, &self.user_name)
                    .await
                {
                    Ok(summary) => activity.digest = Some(summary),
                    Err(err) => {
                        tracing::warn!(
                            "digest: could not summarize {}: {}",
                            terminal_field(&label),
                            terminal_field(&err.to_string())
                        );
                        activity.error = Some(format!("summary failed: {err}"));
                    }
                }
            }
            activities.push(activity);
        }

        build_report(
            &activities,
            self.selection.quiet,
            ReportMeta {
                generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                window: window_label(self.window),
                model: backend.map(SummarizerBackend::label),
                user_name: self.user_name,
            },
        )
    }
}

/// One channel's fetched history.
#[derive(Debug, Default)]
struct History {
    limit_note: Option<String>,
    messages: Vec<MessageRow>,
    /// Ids of the messages whose mention list names the signed-in user.
    mention_ids: HashSet<String>,
}

pub enum HistorySource {
    Mock,
    #[cfg(feature = "experimental-discord")]
    Discord(std::sync::Arc<diself::Client>),
}

impl HistorySource {
    #[cfg_attr(
        not(feature = "experimental-discord"),
        expect(
            clippy::unused_async,
            clippy::unused_async_trait_impl,
            reason = "only the Discord source awaits"
        )
    )]
    async fn history(&self, channel_id: &str, cutoff_ms: u64) -> Result<History> {
        match self {
            Self::Mock => Ok(History {
                messages: messages_in_window(mock::messages(channel_id), cutoff_ms),
                ..History::default()
            }),
            #[cfg(feature = "experimental-discord")]
            Self::Discord(client) => {
                let http = client.http();
                let user = client.cache().current_user();
                let fetch = |before| async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let mut url = format!(
                        "https://discord.com/api/v10/channels/{channel_id}/messages?limit={PAGE_SIZE}"
                    );
                    if let Some(cursor) = before {
                        let _ = write!(url, "&before={cursor}");
                    }
                    http.get(&url)
                        .await
                        .map_err(|err| eyre!("failed to load history: {err}"))
                };
                let pages = history_pages(
                    channel_id,
                    cutoff_ms,
                    user.as_ref().map(|u| u.id.as_str()),
                    fetch,
                );
                let mut history = tokio::time::timeout(Duration::from_secs(45), pages)
                    .await
                    .wrap_err("history request timed out")??;
                let _ = tokio::time::timeout(
                    Duration::from_secs(3),
                    crate::transport::discord::resolve_rows_user_mentions(
                        http,
                        client.cache(),
                        &mut history.messages,
                    ),
                )
                .await;
                Ok(history)
            }
        }
    }
}

#[cfg(feature = "experimental-discord")]
const PAGE_SIZE: usize = 100;

#[cfg(feature = "experimental-discord")]
async fn history_pages<Fut>(
    channel_id: &str,
    cutoff_ms: u64,
    user_id: Option<&str>,
    mut fetch: impl FnMut(Option<u64>) -> Fut,
) -> Result<History>
where
    Fut: Future<Output = Result<serde_json::Value>>,
{
    const MAX_PAGES: usize = 20;
    let mut before = None;
    let mut history = History::default();
    for page in 1..=MAX_PAGES {
        let value = fetch(before).await?;
        let count = value
            .as_array()
            .ok_or_else(|| eyre!("invalid history page"))?
            .len();
        if count > PAGE_SIZE {
            return Err(eyre!("oversized history page"));
        }
        if let Some(id) = user_id {
            history
                .mention_ids
                .extend(crate::transport::discord::ids_of_messages_mentioning(
                    &value, id,
                ));
        }
        let rows = crate::transport::discord::history_rows_from_value(channel_id, value)?;
        // Decode yields oldest first within each page. Walk backwards so the
        // cursor and the collected messages stay newest first across pages.
        let mut crossed_cutoff = false;
        for row in rows.into_iter().rev() {
            let id: u64 = row.id.parse().wrap_err("invalid history cursor")?;
            if before.is_some_and(|cursor| id >= cursor) {
                return Err(eyre!("history cursor did not advance"));
            }
            before = Some(id);
            if (id >> 22) + DISCORD_EPOCH_MS < cutoff_ms {
                crossed_cutoff = true;
                break;
            }
            history.messages.push(row);
        }
        if crossed_cutoff || count < PAGE_SIZE {
            break;
        }
        if page == MAX_PAGES {
            history.limit_note = Some(format!(
                "History may be incomplete ({} message limit)",
                MAX_PAGES * PAGE_SIZE
            ));
        }
    }
    history.messages.reverse();
    Ok(history)
}

fn window_cutoff(window: Duration, now_ms: u64) -> u64 {
    now_ms.saturating_sub(u64::try_from(window.as_millis()).unwrap_or(u64::MAX))
}

/// A channel chosen for the digest, before its history is fetched.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub guild_id: String,
    pub guild_name: String,
    pub channel: ChannelSummary,
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub candidates: Vec<Candidate>,
    /// Servers with a loaded channel list that matched the filters but had no active channel.
    pub quiet: Vec<String>,
}

fn guild_matches(guild: &GuildSummary, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let name = guild.name.to_lowercase();
    filters
        .iter()
        .any(|filter| guild.id == *filter || name.contains(&filter.to_lowercase()))
}

fn last_message_ms(channel: &ChannelSummary) -> Option<u64> {
    channel
        .last_message_id
        .as_deref()
        .and_then(snowflake_timestamp_ms)
}

/// A channel is active when its newest message falls inside the window. Without a
/// usable snowflake (fixtures, channels Discord sent no id for) unread state decides.
fn is_active(channel: &ChannelSummary, cutoff_ms: u64) -> bool {
    last_message_ms(channel).map_or(channel.unread, |timestamp| timestamp >= cutoff_ms)
}

fn channel_rank(channel: &ChannelSummary) -> (u32, bool, u64) {
    (
        channel.unread_count,
        channel.unread,
        last_message_ms(channel).unwrap_or(0),
    )
}

fn guild_rank<'a>(candidates: impl Iterator<Item = &'a Candidate>) -> (u32, u64) {
    candidates.fold((0, 0), |(mentions, newest), candidate| {
        (
            u32::saturating_add(mentions, candidate.channel.unread_count),
            u64::max(newest, last_message_ms(&candidate.channel).unwrap_or(0)),
        )
    })
}

/// Unread mentions in a server channel. A direct message counts every unread
/// message here, so it follows the ordinary ranking.
fn has_unread_mentions(channel: &ChannelSummary) -> bool {
    channel.unread_count > 0 && channel.kind != ChannelKind::DirectMessage
}

fn candidates_of(guild: &GuildSummary, channels: Vec<&ChannelSummary>) -> Vec<Candidate> {
    channels
        .into_iter()
        .map(|channel| Candidate {
            guild_id: guild.id.clone(),
            guild_name: guild.name.clone(),
            channel: channel.clone(),
        })
        .collect()
}

/// Choose which channels to fetch. A channel the whitelist names, or one with
/// unread mentions, is always fetched. The rest are the unmuted channels of the
/// whitelisted servers that are active inside the window: the top few per
/// server, capped overall, mentions first and then recency.
pub fn select_candidates(
    guilds: &[GuildSummary],
    channels_by_guild: &HashMap<String, Vec<ChannelSummary>>,
    options: &DigestOptions,
    now_ms: u64,
) -> Selection {
    let cutoff_ms = window_cutoff(options.window, now_ms);
    // Per server: the channels that are always fetched, then the ranked rest.
    let mut per_guild: Vec<(Vec<Candidate>, Vec<Candidate>)> = Vec::new();
    let mut quiet = Vec::new();

    for guild in guilds {
        if !guild_matches(guild, &options.guild_filters) {
            continue;
        }
        let Some(channels) = channels_by_guild.get(&guild.id) else {
            continue;
        };
        let scope = Scope::of(guild, &options.include);
        let (always, mut ranked): (Vec<&ChannelSummary>, Vec<&ChannelSummary>) = channels
            .iter()
            .filter(|channel| channel.kind.is_selectable() && is_active(channel, cutoff_ms))
            .partition(|channel| scope.names(channel) || has_unread_mentions(channel));
        ranked.retain(|channel| scope.whole_server && !channel.muted);
        if always.is_empty() && ranked.is_empty() {
            if !scope.admits_nothing() {
                quiet.push(guild.name.clone());
            }
            continue;
        }
        ranked.sort_by_key(|channel| std::cmp::Reverse(channel_rank(channel)));
        ranked.truncate(options.top_per_server.saturating_sub(always.len()));
        per_guild.push((candidates_of(guild, always), candidates_of(guild, ranked)));
    }

    per_guild
        .sort_by_key(|(always, ranked)| std::cmp::Reverse(guild_rank(always.iter().chain(ranked))));
    let (always, ranked): (Vec<_>, Vec<_>) = per_guild.into_iter().unzip();
    let mut candidates: Vec<Candidate> = always.into_iter().flatten().collect();
    let room = options.max_channels.saturating_sub(candidates.len());
    candidates.extend(ranked.into_iter().flatten().take(room));
    Selection { candidates, quiet }
}

/// Keep messages posted inside the window. Ids that are not snowflakes are kept.
pub fn messages_in_window(messages: Vec<MessageRow>, cutoff_ms: u64) -> Vec<MessageRow> {
    messages
        .into_iter()
        .filter(|message| {
            snowflake_timestamp_ms(&message.id).is_none_or(|timestamp| timestamp >= cutoff_ms)
        })
        .collect()
}

/// One fetched channel with its optional AI summary.
#[derive(Debug, Clone)]
pub struct ChannelActivity {
    pub candidate: Candidate,
    pub messages: Vec<MessageRow>,
    /// Ids of the messages whose mention list names the signed-in user.
    pub mention_ids: HashSet<String>,
    pub digest: Option<ChannelDigest>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReportMeta {
    pub generated_at: String,
    pub window: String,
    pub model: Option<String>,
    pub user_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DigestReport {
    pub generated_at: String,
    pub window: String,
    pub model: Option<String>,
    pub needs_you: Vec<NeedsYou>,
    pub servers: Vec<ServerDigest>,
    pub quiet: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NeedsYou {
    pub guild: String,
    pub channel: String,
    pub channel_id: String,
    pub author: String,
    pub time: String,
    pub snippet: String,
    pub reason: String,
    pub message_id: String,
    /// The message mentions the user, directly or as a reply that pings them.
    pub mention: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerDigest {
    pub id: String,
    pub name: String,
    pub messages: usize,
    pub channels: Vec<ChannelEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub messages: usize,
    pub mentions: u32,
    pub headline: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
}

impl ChannelEntry {
    /// The one-line headline, or why there is none.
    pub fn headline_or_error(&self) -> Option<&str> {
        self.headline.as_deref().or(self.error.as_deref())
    }
}

#[derive(Clone, Copy)]
pub enum DigestRow<'a> {
    NeedsYou(&'a NeedsYou),
    Channel {
        server: &'a ServerDigest,
        channel: &'a ChannelEntry,
        first_in_server: bool,
    },
    Quiet(&'a [String]),
}

impl DigestRow<'_> {
    pub const fn channel_id(&self) -> Option<&str> {
        match self {
            Self::NeedsYou(item) => Some(item.channel_id.as_str()),
            Self::Channel { channel, .. } => Some(channel.id.as_str()),
            Self::Quiet(_) => None,
        }
    }
}

impl DigestReport {
    pub fn rows(&self) -> impl Iterator<Item = DigestRow<'_>> {
        let channels = self.servers.iter().flat_map(|server| {
            server
                .channels
                .iter()
                .enumerate()
                .map(move |(index, channel)| DigestRow::Channel {
                    server,
                    channel,
                    first_in_server: index == 0,
                })
        });
        self.needs_you
            .iter()
            .map(DigestRow::NeedsYou)
            .chain(channels)
            .chain((!self.quiet.is_empty()).then_some(DigestRow::Quiet(&self.quiet)))
    }

    /// `needs_you` split in two: mentions of the user, which come first, and the rest.
    pub fn mentions_and_others(&self) -> (&[NeedsYou], &[NeedsYou]) {
        let mentions = self
            .needs_you
            .iter()
            .take_while(|item| item.mention)
            .count();
        self.needs_you.split_at(mentions)
    }

    pub fn channel_count(&self) -> usize {
        self.servers
            .iter()
            .map(|server| server.channels.len())
            .sum()
    }
}

fn snippet(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or_default().trim();
    if first_line.chars().count() <= SNIPPET_MAX_CHARS {
        return first_line.to_string();
    }
    let mut cut: String = first_line.chars().take(SNIPPET_MAX_CHARS - 1).collect();
    cut.push('\u{2026}');
    cut
}

fn needs_you_item(
    candidate: &Candidate,
    author: &str,
    time: &str,
    text: &str,
    reason: &str,
    message_id: &str,
) -> NeedsYou {
    NeedsYou {
        guild: candidate.guild_name.clone(),
        channel: format!(
            "{}{}",
            candidate.channel.kind.marker(),
            candidate.channel.name
        ),
        channel_id: candidate.channel.id.clone(),
        author: author.to_string(),
        time: time.to_string(),
        snippet: snippet(text),
        reason: reason.to_string(),
        message_id: message_id.to_string(),
        mention: false,
    }
}

/// Items the AI flagged, with the message time looked up by id.
fn items_from_digest(activity: &ChannelActivity, digest: &ChannelDigest) -> Vec<NeedsYou> {
    digest
        .todos
        .iter()
        .map(|todo| {
            let time = activity
                .messages
                .iter()
                .find(|message| message.id == todo.message_id)
                .map_or("", |message| message.timestamp.as_str());
            needs_you_item(
                &activity.candidate,
                &todo.author,
                time,
                &todo.snippet,
                &todo.reason,
                &todo.message_id,
            )
        })
        .collect()
}

/// Messages that mention the user: the ones Discord lists them on, plus any
/// that spell out their `@name`.
fn mention_items(activity: &ChannelActivity, user_name: &str) -> Vec<NeedsYou> {
    let needle = format!("@{}", user_name.to_lowercase());
    activity
        .messages
        .iter()
        .filter(|message| {
            message.author != user_name
                && (activity.mention_ids.contains(&message.id)
                    || message.content.to_lowercase().contains(&needle))
        })
        .map(|message| NeedsYou {
            mention: true,
            ..needs_you_item(
                &activity.candidate,
                &message.author,
                &message.timestamp,
                &message.content,
                "Mentions you",
                &message.id,
            )
        })
        .collect()
}

/// A direct message whose last word is not the user's.
fn unanswered_dm(activity: &ChannelActivity, user_name: &str) -> Option<NeedsYou> {
    if activity.candidate.channel.kind != ChannelKind::DirectMessage {
        return None;
    }
    let last = activity
        .messages
        .last()
        .filter(|last| last.author != user_name)?;
    Some(needs_you_item(
        &activity.candidate,
        &last.author,
        &last.timestamp,
        &last.content,
        "Unanswered direct message",
        &last.id,
    ))
}

/// Mentions always, then what the AI flagged. Without a summary, an unanswered
/// direct message stands in when nothing in it mentions the user.
fn needs_you_items(activity: &ChannelActivity, user_name: &str) -> Vec<NeedsYou> {
    let mut items = mention_items(activity, user_name);
    let flagged = match &activity.digest {
        Some(digest) => items_from_digest(activity, digest),
        None if items.is_empty() => unanswered_dm(activity, user_name).into_iter().collect(),
        None => Vec::new(),
    };
    for item in flagged {
        match items
            .iter_mut()
            .find(|known| known.message_id == item.message_id)
        {
            // The AI's reason says more than "Mentions you".
            Some(mention) => mention.reason = item.reason,
            None => items.push(item),
        }
    }
    items
}

fn channel_entry(activity: &ChannelActivity) -> ChannelEntry {
    let channel = &activity.candidate.channel;
    ChannelEntry {
        id: channel.id.clone(),
        name: format!("{}{}", channel.kind.marker(), channel.name),
        kind: channel.kind.label(),
        messages: activity.messages.len(),
        mentions: channel.unread_count,
        headline: activity
            .digest
            .as_ref()
            .map(|digest| digest.headline.clone()),
        summary: activity
            .digest
            .as_ref()
            .map(|digest| digest.summary.clone()),
        error: activity.error.clone(),
    }
}

/// Shape fetched activity into the final report. Empty channels are dropped unless
/// their history could not be loaded. A server left with none is listed as quiet.
pub fn build_report(
    activities: &[ChannelActivity],
    quiet: Vec<String>,
    meta: ReportMeta,
) -> DigestReport {
    let mut needs_you = Vec::new();
    let mut servers: Vec<ServerDigest> = Vec::new();
    let mut quiet = quiet;

    for activity in activities {
        let candidate = &activity.candidate;
        if activity.messages.is_empty() && activity.error.is_none() {
            if !quiet.contains(&candidate.guild_name) {
                quiet.push(candidate.guild_name.clone());
            }
            continue;
        }

        needs_you.extend(needs_you_items(activity, &meta.user_name));

        let entry = channel_entry(activity);
        match servers
            .iter_mut()
            .find(|server| server.id == candidate.guild_id)
        {
            Some(server) => {
                server.messages += entry.messages;
                server.channels.push(entry);
            }
            None => servers.push(ServerDigest {
                id: candidate.guild_id.clone(),
                name: candidate.guild_name.clone(),
                messages: entry.messages,
                channels: vec![entry],
            }),
        }
    }

    for server in &mut servers {
        server
            .channels
            .sort_by_key(|channel| std::cmp::Reverse(channel.messages));
    }
    servers.sort_by_key(|server| std::cmp::Reverse(server.messages));
    quiet.retain(|name| !servers.iter().any(|server| server.name == *name));
    needs_you.sort_by_key(|item| !item.mention);

    DigestReport {
        generated_at: meta.generated_at,
        window: meta.window,
        model: meta.model,
        needs_you,
        servers,
        quiet,
    }
}

/// Keep terminal control bytes and field separators out of user-controlled text.
pub fn terminal_field(text: &str) -> Cow<'_, str> {
    if text.chars().any(char::is_control) {
        Cow::Owned(text.replace(char::is_control, " "))
    } else {
        Cow::Borrowed(text)
    }
}

struct Painter {
    color: bool,
}

impl Painter {
    fn paint(&self, code: &str, text: &str) -> String {
        let text = terminal_field(text);
        if self.color {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.into_owned()
        }
    }

    fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    fn accent(&self, text: &str) -> String {
        self.paint("1;33", text)
    }
}

fn pad(text: &str, width: usize) -> String {
    let text = terminal_field(text);
    let padding = width.saturating_sub(UnicodeWidthStr::width(text.as_ref()));
    format!("{text}{}", " ".repeat(padding))
}

pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn header_line(report: &DigestReport) -> String {
    let mut parts = vec![report.generated_at.clone(), report.window.clone()];
    if let Some(model) = &report.model {
        parts.push(model.clone());
    }
    parts.join(" \u{b7} ")
}

/// Render for a terminal. `color` adds ANSI styling.
pub fn render_text(report: &DigestReport, color: bool) -> String {
    let painter = Painter { color };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {}",
        painter.bold("Digest"),
        painter.dim(&format!("\u{b7} {}", header_line(report)))
    );

    if report.servers.is_empty() {
        let _ = writeln!(
            out,
            "\nNothing new in the {}.",
            terminal_field(&report.window)
        );
        return out;
    }

    let (mentions, others) = report.mentions_and_others();
    write_text_items(&mut out, &painter, "MENTIONS", mentions);
    write_text_items(&mut out, &painter, "NEEDS YOU", others);

    let _ = writeln!(out, "\n{}", painter.dim("TOP CHANNELS"));
    for server in &report.servers {
        let _ = writeln!(
            out,
            "{}  {}",
            painter.bold(&server.name),
            painter.dim(&plural(server.messages, "message"))
        );
        let name_width = column_width(server.channels.iter().map(|channel| channel.name.as_str()));
        for channel in &server.channels {
            // Errors can be long; the full text stays in the JSON and the log.
            let detail = snippet(channel.headline_or_error().unwrap_or_default());
            let line = format!(
                "  {}  {:>3}  {}",
                pad(&channel.name, name_width),
                channel.messages,
                painter.dim(&detail)
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
    }

    if !report.quiet.is_empty() {
        let _ = writeln!(
            out,
            "\n{}  {}",
            painter.dim("QUIET"),
            painter.dim(&report.quiet.join(", "))
        );
    }
    out
}

fn write_text_items(out: &mut String, painter: &Painter, title: &str, items: &[NeedsYou]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\n{}",
        painter.accent(&format!("{title}  {}", items.len()))
    );
    let guild_width = column_width(items.iter().map(|item| item.guild.as_str()));
    let channel_width = column_width(items.iter().map(|item| item.channel.as_str()));
    for item in items {
        let who = format!("{} {}", item.author, item.time);
        let _ = writeln!(
            out,
            "  {}  {}  {}  \u{201c}{}\u{201d}",
            painter.dim(&pad(&item.guild, guild_width)),
            painter.bold(&pad(&item.channel, channel_width)),
            painter.dim(who.trim()),
            terminal_field(&item.snippet)
        );
        let _ = writeln!(out, "      {}", painter.dim(&item.reason));
    }
}

fn write_markdown_items(out: &mut String, title: &str, items: &[NeedsYou]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## {title}\n");
    for item in items {
        let who = format!("{} {}", item.author, item.time);
        let _ = writeln!(
            out,
            "- **{} {}** \u{b7} {} \u{b7} {}",
            terminal_field(&item.guild),
            terminal_field(&item.channel),
            terminal_field(who.trim()),
            terminal_field(&item.reason)
        );
        let quote = terminal_field(&item.snippet);
        if !quote.trim().is_empty() {
            let _ = writeln!(out, "  > {}", quote.trim());
        }
    }
}

/// One section per server, with a heading per channel so a long digest can be skimmed.
fn write_markdown_server(out: &mut String, server: &ServerDigest) {
    let _ = writeln!(
        out,
        "\n## {} \u{b7} {}",
        terminal_field(&server.name),
        plural(server.messages, "message")
    );
    let summarized = server.channels.iter().any(|channel| {
        channel.headline.is_some() || channel.summary.is_some() || channel.error.is_some()
    });
    if !summarized {
        // Without summaries a heading per channel would have nothing under it.
        out.push('\n');
        for channel in &server.channels {
            let _ = writeln!(
                out,
                "- **{}** \u{b7} {}",
                terminal_field(&channel.name),
                plural(channel.messages, "message")
            );
        }
        return;
    }
    for channel in &server.channels {
        let _ = writeln!(
            out,
            "\n### {} \u{b7} {}",
            terminal_field(&channel.name),
            plural(channel.messages, "message")
        );
        match (&channel.headline, &channel.error) {
            (Some(headline), Some(_) | None) => {
                let headline = terminal_field(headline);
                if !headline.trim().is_empty() {
                    let _ = writeln!(out, "\n**{}**", headline.trim());
                }
            }
            (None, Some(error)) => {
                let _ = writeln!(out, "\n*{}*", terminal_field(error).trim());
            }
            (None, None) => {}
        }
        if let Some(summary) = &channel.summary {
            let _ = writeln!(out, "\n{}", terminal_field(summary).trim());
        }
    }
}

fn column_width<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    values
        .map(|value| UnicodeWidthStr::width(terminal_field(value).as_ref()))
        .max()
        .unwrap_or(0)
}

/// Render as Markdown, for notes apps and scheduled files.
pub fn render_markdown(report: &DigestReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Discord digest: {}",
        terminal_field(&report.generated_at)
    );

    if report.servers.is_empty() {
        let _ = writeln!(
            out,
            "\nNothing new in the {}.",
            terminal_field(&report.window)
        );
        return out;
    }

    let messages = report.servers.iter().map(|server| server.messages).sum();
    let _ = write!(
        out,
        "\n{} in {} across {} over the {}.",
        plural(messages, "message"),
        plural(report.channel_count(), "channel"),
        plural(report.servers.len(), "server"),
        terminal_field(&report.window)
    );
    if let Some(model) = &report.model {
        let _ = write!(out, " Summaries by {}.", terminal_field(model));
    }
    out.push('\n');

    let (mentions, others) = report.mentions_and_others();
    write_markdown_items(&mut out, "Mentions", mentions);
    write_markdown_items(&mut out, "Needs you", others);

    for server in &report.servers {
        write_markdown_server(&mut out, server);
    }

    if !report.quiet.is_empty() {
        let _ = writeln!(
            out,
            "\n## Quiet\n\n{}",
            terminal_field(&report.quiet.join(", "))
        );
    }
    out
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::digest::*;
    use crate::model::TodoItem;

    const HOUR_MS: u64 = 3_600_000;
    const NOW_MS: u64 = 1_795_000_000_000;

    fn snowflake_at(ms: u64) -> String {
        ((ms - DISCORD_EPOCH_MS) << 22).to_string()
    }

    fn guild(id: &str, name: &str) -> GuildSummary {
        GuildSummary {
            id: id.into(),
            name: name.into(),
            muted: false,
            unread: false,
            unread_count: 0,
            avatar_url: None,
        }
    }

    fn channel(id: &str, name: &str, last_ms: Option<u64>, mentions: u32) -> ChannelSummary {
        ChannelSummary {
            id: id.into(),
            guild_id: None,
            parent_id: None,
            name: name.into(),
            kind: ChannelKind::Text,
            position: 0,
            muted: false,
            unread: mentions > 0,
            unread_count: mentions,
            last_message_id: last_ms.map(snowflake_at),
        }
    }

    fn message(id: &str, author: &str, content: &str) -> MessageRow {
        MessageRow {
            id: id.into(),
            channel_id: "c1".into(),
            author: author.into(),
            author_avatar_url: None,
            content: content.into(),
            attachments: Vec::new(),
            timestamp: "10:35".into(),
            edited: false,
            is_continuation: false,
        }
    }

    fn candidate(guild_id: &str, guild_name: &str, channel: ChannelSummary) -> Candidate {
        Candidate {
            guild_id: guild_id.into(),
            guild_name: guild_name.into(),
            channel,
        }
    }

    fn activity(candidate: Candidate, messages: Vec<MessageRow>) -> ChannelActivity {
        ChannelActivity {
            candidate,
            messages,
            mention_ids: HashSet::new(),
            digest: None,
            error: None,
        }
    }

    fn meta() -> ReportMeta {
        ReportMeta {
            generated_at: "Sat Nov 21".into(),
            window: "last 24h".into(),
            model: None,
            user_name: "ferris".into(),
        }
    }

    #[test]
    fn snowflake_round_trips_through_timestamp() {
        assert_eq!(
            snowflake_timestamp_ms(&snowflake_at(NOW_MS)).unwrap(),
            NOW_MS
        );
        assert_eq!(snowflake_timestamp_ms("m12"), None);
        assert_eq!(snowflake_timestamp_ms(""), None);
    }

    #[test]
    fn window_parses_units_and_rejects_garbage() {
        assert_eq!(parse_window("24h").unwrap(), Duration::from_hours(24));
        assert_eq!(parse_window("90m").unwrap(), Duration::from_mins(90));
        assert_eq!(parse_window("2d").unwrap(), Duration::from_hours(48));
        assert_eq!(parse_window("1w").unwrap(), Duration::from_hours(168));
        for bad in [
            "",
            "h",
            "0h",
            "12",
            "-3h",
            "1.5h",
            "5y",
            "99999999999999999999w",
        ] {
            assert!(parse_window(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn window_label_prefers_hours_under_two_days() {
        assert_eq!(window_label(Duration::from_hours(24)), "last 24h");
        assert_eq!(window_label(Duration::from_hours(48)), "last 2d");
        assert_eq!(window_label(Duration::from_mins(90)), "last 90m");
    }

    #[test]
    fn selection_keeps_channels_inside_the_window() {
        let guilds = vec![guild("g1", "Rust Lang")];
        let channels = HashMap::from([(
            "g1".to_string(),
            vec![
                channel("fresh", "general", Some(NOW_MS - HOUR_MS), 0),
                channel("stale", "old", Some(NOW_MS - 30 * HOUR_MS), 2),
            ],
        )]);

        let selection = select_candidates(&guilds, &channels, &DigestOptions::default(), NOW_MS);
        let ids: Vec<&str> = selection
            .candidates
            .iter()
            .map(|c| c.channel.id.as_str())
            .collect();
        assert_eq!(ids, vec!["fresh"]);
        assert_eq!(selection.quiet, Vec::<String>::new());
    }

    #[test]
    fn selection_skips_muted_channels_and_categories_but_keeps_muted_servers() {
        let mut muted_guild = guild("g2", "Muted");
        muted_guild.muted = true;
        let mut muted_channel = channel("m", "muted", Some(NOW_MS), 0);
        muted_channel.muted = true;
        let mut category = channel("cat", "Category", Some(NOW_MS), 0);
        category.kind = ChannelKind::Category;

        let guilds = vec![guild("g1", "Rust Lang"), muted_guild];
        let channels = HashMap::from([
            ("g1".to_string(), vec![muted_channel, category]),
            ("g2".to_string(), vec![channel("x", "x", Some(NOW_MS), 9)]),
        ]);

        let selection = select_candidates(&guilds, &channels, &DigestOptions::default(), NOW_MS);
        let chosen: Vec<&str> = selection
            .candidates
            .iter()
            .map(|candidate| candidate.channel.id.as_str())
            .collect();
        assert_eq!(chosen, vec!["x"]);
        assert_eq!(selection.quiet, vec!["Rust Lang".to_string()]);
    }

    #[test]
    fn selection_ranks_mentions_first_and_caps_per_server_and_overall() {
        let guilds = vec![guild("g1", "Quiet-ish"), guild("g2", "Busy")];
        let channels = HashMap::from([
            (
                "g1".to_string(),
                vec![channel("a", "a", Some(NOW_MS - HOUR_MS), 0)],
            ),
            (
                "g2".to_string(),
                vec![
                    channel("b1", "b1", Some(NOW_MS - 3 * HOUR_MS), 0),
                    channel("b2", "b2", Some(NOW_MS - 2 * HOUR_MS), 4),
                    channel("b3", "b3", Some(NOW_MS - HOUR_MS), 0),
                ],
            ),
        ]);
        let opts = DigestOptions {
            top_per_server: 2,
            max_channels: 2,
            ..DigestOptions::default()
        };

        let selection = select_candidates(&guilds, &channels, &opts, NOW_MS);
        let ids: Vec<&str> = selection
            .candidates
            .iter()
            .map(|c| c.channel.id.as_str())
            .collect();
        assert_eq!(ids, vec!["b2", "b3"]);
    }

    #[test]
    fn selection_filters_servers_by_name_or_id() {
        let guilds = vec![guild("g1", "Rust Lang"), guild("g2", "Tokio")];
        let channels = HashMap::from([
            ("g1".to_string(), vec![channel("a", "a", Some(NOW_MS), 0)]),
            ("g2".to_string(), vec![channel("b", "b", Some(NOW_MS), 0)]),
        ]);

        let by_name = DigestOptions {
            guild_filters: vec!["tok".into()],
            ..DigestOptions::default()
        };
        let selection = select_candidates(&guilds, &channels, &by_name, NOW_MS);
        assert_eq!(selection.candidates.len(), 1);
        assert_eq!(selection.candidates[0].guild_name, "Tokio");

        let by_id = DigestOptions {
            guild_filters: vec!["g1".into()],
            ..DigestOptions::default()
        };
        let selection = select_candidates(&guilds, &channels, &by_id, NOW_MS);
        assert_eq!(selection.candidates[0].guild_name, "Rust Lang");
    }

    fn chosen(selection: &Selection) -> Vec<&str> {
        selection
            .candidates
            .iter()
            .map(|candidate| candidate.channel.id.as_str())
            .collect()
    }

    fn whitelist(entries: &[&str]) -> Vec<Include> {
        entries
            .iter()
            .filter_map(|entry| Include::parse(entry))
            .collect()
    }

    #[test]
    fn whitelist_entries_name_a_server_or_one_channel() {
        assert_eq!(
            whitelist(&["Rust Lang", " Tokio #Changelog ", "#ops", "", " # "]),
            vec![
                Include {
                    server: "rust lang".into(),
                    channel: None,
                },
                Include {
                    server: "tokio".into(),
                    channel: Some("changelog".into()),
                },
                Include {
                    server: String::new(),
                    channel: Some("ops".into()),
                },
            ]
        );
    }

    #[test]
    fn whitelist_keeps_listed_servers_and_named_channels_only() {
        let mut changelog = channel("log", "changelog", Some(NOW_MS), 0);
        changelog.muted = true;
        let guilds = vec![
            guild("g1", "Rust Lang"),
            guild("g2", "Tokio"),
            guild("g3", "ratatui"),
        ];
        let channels = HashMap::from([
            (
                "g1".to_string(),
                vec![
                    channel("gen", "general", Some(NOW_MS), 0),
                    channel("ios", "ios", Some(NOW_MS - HOUR_MS), 0),
                ],
            ),
            (
                "g2".to_string(),
                vec![changelog, channel("help", "help", Some(NOW_MS), 0)],
            ),
            (
                "g3".to_string(),
                vec![channel("daily", "daily-theme", Some(NOW_MS), 0)],
            ),
        ]);
        let opts = DigestOptions {
            include: whitelist(&["rust lang", "Tokio #changelog"]),
            ..DigestOptions::default()
        };

        let selection = select_candidates(&guilds, &channels, &opts, NOW_MS);
        // The named channel is fetched first, muted or not; then the listed server.
        assert_eq!(chosen(&selection), vec!["log", "gen", "ios"]);
        assert_eq!(selection.quiet, Vec::<String>::new());
    }

    #[test]
    fn whitelisted_server_with_nothing_new_is_quiet_but_others_are_left_out() {
        let guilds = vec![guild("g1", "Tokio"), guild("g2", "ratatui")];
        let stale = Some(NOW_MS - 30 * HOUR_MS);
        let channels = HashMap::from([
            (
                "g1".to_string(),
                vec![
                    channel("log", "changelog", stale, 0),
                    channel("help", "help", Some(NOW_MS), 0),
                ],
            ),
            ("g2".to_string(), vec![channel("old", "old", stale, 0)]),
        ]);
        let opts = DigestOptions {
            include: whitelist(&["Tokio #changelog"]),
            ..DigestOptions::default()
        };

        let selection = select_candidates(&guilds, &channels, &opts, NOW_MS);
        assert_eq!(chosen(&selection), Vec::<&str>::new());
        assert_eq!(selection.quiet, vec!["Tokio".to_string()]);
    }

    #[test]
    fn channels_with_unread_mentions_are_fetched_past_the_whitelist_mutes_and_caps() {
        let mut muted = channel("muted", "muted", Some(NOW_MS), 2);
        muted.muted = true;
        let mut dm = channel("dm", "alice", Some(NOW_MS), 3);
        dm.kind = ChannelKind::DirectMessage;
        let guilds = vec![
            guild("g1", "Listed"),
            guild("g2", "Unlisted"),
            guild("@me", "Direct Messages"),
        ];
        let channels = HashMap::from([
            ("g1".to_string(), vec![channel("a", "a", Some(NOW_MS), 0)]),
            (
                "g2".to_string(),
                vec![
                    muted,
                    channel("pinged", "pinged", Some(NOW_MS), 1),
                    channel("plain", "plain", Some(NOW_MS), 0),
                ],
            ),
            ("@me".to_string(), vec![dm]),
        ]);
        let opts = DigestOptions {
            include: whitelist(&["listed"]),
            top_per_server: 1,
            max_channels: 1,
            ..DigestOptions::default()
        };

        let selection = select_candidates(&guilds, &channels, &opts, NOW_MS);
        assert_eq!(chosen(&selection), vec!["muted", "pinged"]);
    }

    #[test]
    fn channels_without_snowflakes_fall_back_to_unread_state() {
        let guilds = vec![guild("g1", "Fixtures")];
        let channels = HashMap::from([(
            "g1".to_string(),
            vec![
                channel("read", "read", None, 0),
                channel("unread", "unread", None, 1),
            ],
        )]);

        let selection = select_candidates(&guilds, &channels, &DigestOptions::default(), NOW_MS);
        assert_eq!(selection.candidates.len(), 1);
        assert_eq!(selection.candidates[0].channel.id, "unread");
    }

    #[test]
    fn window_filter_drops_old_snowflakes_and_keeps_fixture_ids() {
        let cutoff = NOW_MS - 24 * HOUR_MS;
        let kept = messages_in_window(
            vec![
                message(&snowflake_at(NOW_MS - 30 * HOUR_MS), "a", "old"),
                message(&snowflake_at(NOW_MS - HOUR_MS), "a", "new"),
                message("m1", "a", "fixture"),
            ],
            cutoff,
        );
        let contents: Vec<&str> = kept.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, vec!["new", "fixture"]);
    }

    #[test]
    fn report_orders_servers_and_channels_by_message_count() {
        let small = activity(
            candidate("g1", "Rust Lang", channel("c1", "general", None, 1)),
            vec![message("m1", "alice", "hi")],
        );
        let big = activity(
            candidate("g2", "Tokio", channel("c2", "help", None, 1)),
            vec![message("t1", "carl", "one"), message("t2", "sean", "two")],
        );
        let bigger = activity(
            candidate("g2", "Tokio", channel("c3", "general", None, 1)),
            vec![
                message("t3", "carl", "one"),
                message("t4", "sean", "two"),
                message("t5", "sean", "three"),
            ],
        );

        let report = build_report(&[small, big, bigger], Vec::new(), meta());
        assert_eq!(report.servers[0].name, "Tokio");
        assert_eq!(report.servers[0].messages, 5);
        assert_eq!(report.servers[0].channels[0].name, "#general");
        assert_eq!(report.servers[1].name, "Rust Lang");
        assert_eq!(report.channel_count(), 3);
    }

    #[test]
    fn server_with_no_messages_in_window_is_reported_quiet() {
        let empty = activity(
            candidate("g1", "Rust Lang", channel("c1", "general", None, 1)),
            Vec::new(),
        );
        let report = build_report(&[empty], vec!["ratatui".into()], meta());
        assert_eq!(report.servers, Vec::<ServerDigest>::new());
        assert_eq!(report.quiet, vec!["ratatui", "Rust Lang"]);
    }

    #[test]
    fn mentions_need_you_without_an_ai_backend() {
        let act = activity(
            candidate("g1", "Rust Lang", channel("c1", "general", None, 1)),
            vec![
                message("m11", "charlie", "unrelated"),
                message("m12", "dave", "@Ferris thoughts on the new edition?"),
                message("m13", "ferris", "replying to @ferris myself"),
            ],
        );

        let report = build_report(&[act], Vec::new(), meta());
        assert_eq!(report.needs_you.len(), 1);
        let item = &report.needs_you[0];
        assert_eq!(item.author, "dave");
        assert_eq!(item.channel, "#general");
        assert_eq!(item.message_id, "m12");
        assert_eq!(item.reason, "Mentions you");
    }

    #[test]
    fn unanswered_dm_needs_you_but_answered_dm_does_not() {
        let mut dm = channel("dm1", "alice", None, 1);
        dm.kind = ChannelKind::DirectMessage;
        let unanswered = activity(
            candidate("@me", "Direct Messages", dm.clone()),
            vec![message("d1", "alice", "Pushed.")],
        );
        let answered = activity(
            candidate("@me", "Direct Messages", dm),
            vec![
                message("d1", "alice", "Pushed."),
                message("d2", "ferris", "Thanks"),
            ],
        );

        let report = build_report(&[unanswered], Vec::new(), meta());
        assert_eq!(report.needs_you.len(), 1);
        assert_eq!(report.needs_you[0].channel, "@alice");
        assert_eq!(report.needs_you[0].reason, "Unanswered direct message");

        let report = build_report(&[answered], Vec::new(), meta());
        assert_eq!(report.needs_you, Vec::<NeedsYou>::new());
    }

    #[test]
    fn ai_action_items_replace_heuristics_and_carry_message_time() {
        let mut act = activity(
            candidate("g1", "Rust Lang", channel("c1", "general", None, 1)),
            vec![message("m12", "dave", "@ferris thoughts?")],
        );
        act.digest = Some(ChannelDigest {
            channel_id: "c1".into(),
            headline: "Edition talk".into(),
            summary: "People compared notes.".into(),
            todos: vec![TodoItem {
                author: "dave".into(),
                snippet: "thoughts?".into(),
                reason: "Direct question to you".into(),
                message_id: "m12".into(),
            }],
            generated_at: "10:40".into(),
        });

        let report = build_report(&[act], Vec::new(), meta());
        assert_eq!(report.needs_you.len(), 1);
        assert_eq!(report.needs_you[0].reason, "Direct question to you");
        assert!(report.needs_you[0].mention);
        assert_eq!(report.needs_you[0].time, "10:35");
        let entry = &report.servers[0].channels[0];
        assert_eq!(entry.headline.as_deref(), Some("Edition talk"));
    }

    fn summarized(mut act: ChannelActivity, todo_message_id: &str) -> ChannelActivity {
        act.digest = Some(ChannelDigest {
            channel_id: act.candidate.channel.id.clone(),
            headline: "Release talk".into(),
            summary: "People compared notes.".into(),
            todos: vec![TodoItem {
                author: "erin".into(),
                snippet: "can someone review?".into(),
                reason: "Open review request".into(),
                message_id: todo_message_id.into(),
            }],
            generated_at: "10:40".into(),
        });
        act
    }

    #[test]
    fn a_reply_that_pings_you_is_a_mention_even_with_an_ai_summary() {
        let mut act = activity(
            candidate("g1", "Rust Lang", channel("c1", "general", None, 1)),
            vec![
                message("m1", "erin", "can someone review?"),
                message("m2", "dave", "\u{21a9} works on my machine"),
                message("m3", "ferris", "\u{21a9} noted"),
            ],
        );
        act.mention_ids = HashSet::from(["m2".to_string(), "m3".to_string()]);

        let report = build_report(&[summarized(act, "m1")], Vec::new(), meta());
        let items: Vec<(&str, bool, &str)> = report
            .needs_you
            .iter()
            .map(|item| (item.message_id.as_str(), item.mention, item.reason.as_str()))
            .collect();
        assert_eq!(
            items,
            vec![
                ("m2", true, "Mentions you"),
                ("m1", false, "Open review request"),
            ]
        );
    }

    #[test]
    fn mentions_from_every_channel_come_first_under_their_own_heading() {
        let flagged = summarized(
            activity(
                candidate("g1", "Rust Lang", channel("c1", "general", None, 0)),
                vec![message("m1", "erin", "can someone review?")],
            ),
            "m1",
        );
        let pinged = activity(
            candidate("g2", "Tokio", channel("c2", "help", None, 1)),
            vec![message("m2", "dave", "@ferris is this yours?")],
        );

        let report = build_report(&[flagged, pinged], Vec::new(), meta());
        let (mentions, others) = report.mentions_and_others();
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].message_id, "m2");
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].message_id, "m1");

        let markdown = render_markdown(&report);
        let mentions_at = markdown.find("## Mentions\n\n- **Tokio #help**").unwrap();
        let needs_at = markdown
            .find("## Needs you\n\n- **Rust Lang #general**")
            .unwrap();
        assert!(mentions_at < needs_at);
        let text = render_text(&report, false);
        assert!(text.find("MENTIONS  1").unwrap() < text.find("NEEDS YOU  1").unwrap());
    }

    #[test]
    fn long_snippets_are_cut_on_a_char_boundary() {
        let long = "\u{e9}".repeat(300);
        let cut = snippet(&long);
        assert_eq!(cut.chars().count(), SNIPPET_MAX_CHARS);
        assert!(cut.ends_with('\u{2026}'));
        assert_eq!(snippet("first\nsecond"), "first");
    }

    fn sample_report() -> DigestReport {
        let mut act = activity(
            candidate("g1", "Rust Lang", channel("c1", "general", None, 1)),
            vec![message(
                "m12",
                "dave",
                "@ferris thoughts on the new edition?",
            )],
        );
        act.error = Some("summary failed: timeout".into());
        build_report(&[act], vec!["ratatui".into()], meta())
    }

    #[cfg(feature = "experimental-discord")]
    #[tokio::test]
    async fn history_crosses_pages_to_include_the_cutoff_and_older_mentions() {
        let mut requests = Vec::new();
        let history = history_pages("c", NOW_MS, Some("me"), |before| {
            requests.push(before);
            let offsets = if before.is_none() { 2..102 } else { 0..2 };
            let page: Vec<_> = offsets.rev().map(|i| {
                serde_json::json!({
                    "id": snowflake_at(NOW_MS - 1 + i),
                    "timestamp": "2026-11-22T12:00:00Z",
                    "author": {"id":"1", "username":"reader"},
                    "mentions": if i == 1 { vec![serde_json::json!({"id":"me","username":"me"})] } else { Vec::new() },
                })
            }).collect();
            std::future::ready(Ok(serde_json::Value::Array(page)))
        }).await.unwrap();
        assert_eq!(
            requests,
            [None, Some(snowflake_at(NOW_MS + 1).parse().unwrap())]
        );
        assert_eq!(history.messages.len(), 101);
        assert_eq!(history.messages.first().unwrap().id, snowflake_at(NOW_MS));
        assert_eq!(
            history.messages.last().unwrap().id,
            snowflake_at(NOW_MS + 100)
        );
        assert!(history.mention_ids.contains(&snowflake_at(NOW_MS)));
        assert!(history.limit_note.is_none());
    }

    #[test]
    fn digest_fields_cannot_inject_terminal_controls() {
        let mut report = sample_report();
        let hostile = "hello\x1b]0;title\x07\r\n\t\u{009b}world";
        report.needs_you[0].snippet = hostile.into();
        report.servers[0].name = hostile.into();
        report.servers[0].channels[0].name = hostile.into();
        let plain = render_text(&report, false);
        assert!(!plain.chars().any(|c| c.is_control() && c != '\n'));
        let mut colored = render_text(&report, true);
        for style in ["1", "2", "1;33", "0"] {
            colored = colored.replace(&format!("\x1b[{style}m"), "");
        }
        assert_eq!(colored, plain);
        assert!(!render_markdown(&report).contains('\x1b'));
    }

    #[test]
    fn text_rendering_has_sections_and_no_escapes_without_color() {
        let text = render_text(&sample_report(), false);
        assert!(text.starts_with("Digest \u{b7} Sat Nov 21 \u{b7} last 24h\n"));
        assert!(text.contains("MENTIONS  1"));
        assert!(!text.contains("NEEDS YOU"));
        assert!(text.contains("Rust Lang  1 message\n"));
        assert!(text.contains("#general    1  summary failed: timeout"));
        assert!(text.contains("QUIET  ratatui"));
        assert!(!text.contains('\u{1b}'));
    }

    #[test]
    fn text_rendering_adds_ansi_only_when_asked() {
        assert!(render_text(&sample_report(), true).contains("\u{1b}[1m"));
    }

    #[test]
    fn empty_report_says_nothing_is_new() {
        let report = build_report(&[], vec!["ratatui".into()], meta());
        assert!(render_text(&report, false).contains("Nothing new in the last 24h."));
        assert!(render_markdown(&report).contains("Nothing new in the last 24h."));
    }

    #[test]
    fn markdown_rendering_lists_needs_you_and_channels() {
        let markdown = render_markdown(&sample_report());
        assert!(markdown.starts_with(
            "# Discord digest: Sat Nov 21\n\n\
             1 message in 1 channel across 1 server over the last 24h.\n"
        ));
        assert!(markdown.contains(
            "## Mentions\n\n- **Rust Lang #general** \u{b7} dave 10:35 \u{b7} Mentions you\n  \
             > @ferris thoughts on the new edition?\n"
        ));
        assert!(!markdown.contains("## Needs you"));
        assert!(markdown.contains(
            "\n## Rust Lang \u{b7} 1 message\n\n### #general \u{b7} 1 message\n\n\
             *summary failed: timeout*\n"
        ));
        assert!(markdown.ends_with("\n## Quiet\n\nratatui\n"));
    }

    #[test]
    fn markdown_gives_each_summarized_channel_a_headline_and_a_paragraph() {
        let mut report = sample_report();
        report.model = Some("test-model".into());
        let channel = &mut report.servers[0].channels[0];
        channel.error = None;
        channel.headline = Some("New edition lands".into());
        channel.summary = Some("The team shipped it.\nFeedback is open.".into());
        let markdown = render_markdown(&report);
        assert!(markdown.contains("over the last 24h. Summaries by test-model.\n"));
        assert!(markdown.contains(
            "### #general \u{b7} 1 message\n\n**New edition lands**\n\n\
             The team shipped it. Feedback is open.\n"
        ));

        let channel = &mut report.servers[0].channels[0];
        channel.headline = None;
        channel.summary = None;
        assert!(render_markdown(&report).contains(
            "## Rust Lang \u{b7} 1 message\n\n- **#general** \u{b7} 1 message\n\n## Quiet"
        ));
    }

    #[test]
    fn json_rendering_exposes_needs_you_fields() {
        let json = serde_json::to_value(sample_report()).unwrap();
        assert_eq!(json["needs_you"][0]["guild"], "Rust Lang");
        assert_eq!(json["needs_you"][0]["channel"], "#general");
        assert_eq!(json["needs_you"][0]["message_id"], "m12");
        assert_eq!(json["servers"][0]["channels"][0]["kind"], "text");
        assert_eq!(json["quiet"][0], "ratatui");
    }
}
