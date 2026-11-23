pub mod cache;
pub mod client;
pub mod error;
pub mod gateway;
pub mod http;
pub mod model;

pub use cache::{Cache, CacheConfig};
pub use client::{
    ChannelsManager, Client, ClientBuilder, CollectorHub, CollectorOptions, Context,
    DispatchEvent, DispatchEventType, EventHandler, GuildsManager, MessageCollector,
    ReactionCollectEvent, ReactionCollector, ReactionEventType, RelationshipsManager,
    SearchParams, SearchThreadsParams, UsersManager,
};
pub use error::{CaptchaInfo, Error, Result};
pub use http::HttpClient;
pub use model::{
    Channel, CreateMessage, EmbedBuilder, Message, PassiveChannelState, PassiveUpdateV1,
    ReadStateEntry, ReadySupplemental, User,
};

/// Prelude module for easy imports
///
/// # Example
/// ```
/// use diself::prelude::*;
/// ```
pub mod prelude {
    pub use crate::cache::{Cache, CacheConfig};
    pub use crate::client::{
        ChannelsManager, Client, ClientBuilder, CollectorHub, CollectorOptions, Context,
        DispatchEvent, DispatchEventType, EventHandler, GuildsManager, MessageCollector,
        ReactionCollectEvent, ReactionCollector, ReactionEventType, RelationshipsManager,
        SearchParams, SearchThreadsParams, UsersManager,
    };
    pub use crate::error::{CaptchaInfo, Error, Result};
    pub use crate::http::HttpClient;
    pub use crate::model::{
        Channel, CreateMessage, EmbedBuilder, Message, PassiveChannelState, PassiveUpdateV1,
        ReadStateEntry, ReadySupplemental, User,
    };
    pub use async_trait::async_trait;
}
