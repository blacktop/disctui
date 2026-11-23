use crate::client::{DispatchEvent, DispatchEventType};
use crate::model::{Emoji, Message};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{self, Duration, Instant};

/// Options shared by message/reaction collectors.
///
/// `time` defines the maximum lifetime of the collector.
/// `max` defines how many items can be collected before closing.
///
/// # Example
/// ```ignore
/// use diself::CollectorOptions;
/// use std::time::Duration;
///
/// let opts = CollectorOptions {
///     time: Some(Duration::from_secs(30)),
///     max: Some(10),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct CollectorOptions {
    pub time: Option<Duration>,
    pub max: Option<usize>,
}

impl Default for CollectorOptions {
    fn default() -> Self {
        Self {
            time: Some(Duration::from_secs(30)),
            max: None,
        }
    }
}

/// Internal collector dispatcher fed by gateway dispatch events.
///
/// This hub powers `Context::message_collector(...)` and
/// `Context::reaction_collector(...)`.
#[derive(Clone)]
pub struct CollectorHub {
    tx: broadcast::Sender<DispatchEvent>,
}

impl CollectorHub {
    /// Creates a new collector hub.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    /// Broadcasts one dispatch event to all active collectors.
    pub fn dispatch(&self, event: DispatchEvent) {
        let _ = self.tx.send(event);
    }

    /// Creates a message collector listening to `MESSAGE_CREATE`.
    ///
    /// # Example
    /// ```ignore
    /// use diself::{CollectorOptions, Context};
    /// use std::time::Duration;
    ///
    /// async fn example(ctx: &Context) {
    ///     let mut collector = ctx.message_collector(
    ///         CollectorOptions {
    ///             time: Some(Duration::from_secs(15)),
    ///             max: Some(3),
    ///         },
    ///         |m| m.content.starts_with("!"),
    ///     );
    ///
    ///     while let Some(msg) = collector.next().await {
    ///         println!("Collected: {}", msg.content);
    ///     }
    /// }
    /// ```
    pub fn message_collector<F>(&self, options: CollectorOptions, filter: F) -> MessageCollector
    where
        F: Fn(&Message) -> bool + Send + Sync + 'static,
    {
        let mut rx = self.tx.subscribe();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let filter = Arc::new(filter);

        tokio::spawn(async move {
            let deadline = options.time.map(|t| Instant::now() + t);
            let mut collected = 0usize;

            loop {
                if let Some(max) = options.max {
                    if collected >= max {
                        break;
                    }
                }

                let event = if let Some(deadline) = deadline {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    match time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(evt)) => evt,
                        Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                        Ok(Err(broadcast::error::RecvError::Closed)) => break,
                        Err(_) => break,
                    }
                } else {
                    match rx.recv().await {
                        Ok(evt) => evt,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                };

                if event.kind != DispatchEventType::MessageCreate {
                    continue;
                }

                let Ok(message) = serde_json::from_value::<Message>(event.data.clone()) else {
                    continue;
                };

                if !(filter)(&message) {
                    continue;
                }

                if out_tx.send(message).is_err() {
                    break;
                }
                collected += 1;
            }
        });

        MessageCollector { rx: out_rx }
    }

    /// Creates a reaction collector listening to reaction add/remove dispatches.
    ///
    /// Events supported:
    /// - `MESSAGE_REACTION_ADD`
    /// - `MESSAGE_REACTION_REMOVE`
    pub fn reaction_collector<F>(&self, options: CollectorOptions, filter: F) -> ReactionCollector
    where
        F: Fn(&ReactionCollectEvent) -> bool + Send + Sync + 'static,
    {
        let mut rx = self.tx.subscribe();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let filter = Arc::new(filter);

        tokio::spawn(async move {
            let deadline = options.time.map(|t| Instant::now() + t);
            let mut collected = 0usize;

            loop {
                if let Some(max) = options.max {
                    if collected >= max {
                        break;
                    }
                }

                let event = if let Some(deadline) = deadline {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    match time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(evt)) => evt,
                        Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                        Ok(Err(broadcast::error::RecvError::Closed)) => break,
                        Err(_) => break,
                    }
                } else {
                    match rx.recv().await {
                        Ok(evt) => evt,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                };

                let Some(reaction_event) = ReactionCollectEvent::from_dispatch(&event) else {
                    continue;
                };

                if !(filter)(&reaction_event) {
                    continue;
                }

                if out_tx.send(reaction_event).is_err() {
                    break;
                }
                collected += 1;
            }
        });

        ReactionCollector { rx: out_rx }
    }
}

impl Default for CollectorHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Collector over `Message` values.
///
/// Built through `Context::message_collector(...)`.
pub struct MessageCollector {
    rx: mpsc::UnboundedReceiver<Message>,
}

impl MessageCollector {
    /// Waits for the next collected message.
    pub async fn next(&mut self) -> Option<Message> {
        self.rx.recv().await
    }

    /// Drains all remaining collected messages until the collector closes.
    pub async fn collect(mut self) -> Vec<Message> {
        let mut out = Vec::new();
        while let Some(item) = self.rx.recv().await {
            out.push(item);
        }
        out
    }
}

/// Type of reaction dispatch captured by `ReactionCollector`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionEventType {
    /// Corresponds to `MESSAGE_REACTION_ADD`.
    Add,
    /// Corresponds to `MESSAGE_REACTION_REMOVE`.
    Remove,
}

/// Flattened reaction event passed to `ReactionCollector` consumers.
#[derive(Debug, Clone)]
pub struct ReactionCollectEvent {
    pub kind: ReactionEventType,
    pub channel_id: String,
    pub message_id: String,
    pub user_id: String,
    pub guild_id: Option<String>,
    pub emoji: Emoji,
}

impl ReactionCollectEvent {
    fn from_dispatch(event: &DispatchEvent) -> Option<Self> {
        let kind = match event.kind {
            DispatchEventType::MessageReactionAdd => ReactionEventType::Add,
            DispatchEventType::MessageReactionRemove => ReactionEventType::Remove,
            _ => return None,
        };

        let data = &event.data;
        let channel_id = data.get("channel_id")?.as_str()?.to_string();
        let message_id = data.get("message_id")?.as_str()?.to_string();
        let user_id = data.get("user_id")?.as_str()?.to_string();
        let guild_id = data
            .get("guild_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let emoji = serde_json::from_value::<Emoji>(data.get("emoji")?.clone()).ok()?;

        Some(Self {
            kind,
            channel_id,
            message_id,
            user_id,
            guild_id,
            emoji,
        })
    }
}

/// Collector over `ReactionCollectEvent` values.
///
/// Built through `Context::reaction_collector(...)`.
pub struct ReactionCollector {
    rx: mpsc::UnboundedReceiver<ReactionCollectEvent>,
}

impl ReactionCollector {
    /// Waits for the next collected reaction event.
    pub async fn next(&mut self) -> Option<ReactionCollectEvent> {
        self.rx.recv().await
    }

    /// Drains all remaining collected reaction events until closed.
    pub async fn collect(mut self) -> Vec<ReactionCollectEvent> {
        let mut out = Vec::new();
        while let Some(item) = self.rx.recv().await {
            out.push(item);
        }
        out
    }
}
