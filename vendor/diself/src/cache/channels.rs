use crate::model::{Channel, Guild};
use dashmap::DashMap;
use std::sync::Arc;

/// Cache for channels (channel_id -> Channel)
#[derive(Clone)]
pub struct ChannelCache {
    enabled: bool,
    channels: Arc<DashMap<String, Channel>>,
}

impl ChannelCache {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            channels: Arc::new(DashMap::new()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn get(&self, channel_id: &str) -> Option<Channel> {
        self.channels.get(channel_id).map(|entry| entry.clone())
    }

    pub fn insert(&self, channel: Channel) {
        if self.enabled {
            self.channels.insert(channel.id.clone(), channel);
        }
    }

    pub fn remove(&self, channel_id: &str) -> Option<Channel> {
        self.channels.remove(channel_id).map(|(_, channel)| channel)
    }

    pub fn count(&self) -> usize {
        self.channels.len()
    }

    pub fn all(&self) -> Vec<Channel> {
        self.channels
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn clear(&self) {
        self.channels.clear();
    }

    /// Initializes the channel cache with data from the READY event
    pub fn initialize_from_ready(&self, data: serde_json::Value) {
        if let Some(guilds) = data.as_array() {
            for guild in guilds {
                if let Ok(g) = serde_json::from_value::<Guild>(guild.clone()) {
                    g.channels.iter().for_each(|c| self.insert(c.clone()));
                } else {
                    eprintln!("Failed to deserialize guild for channel cache initialization");
                }
            }
        } else {
            eprintln!("Expected an array of guilds for channel cache initialization");
        }
    }
}
