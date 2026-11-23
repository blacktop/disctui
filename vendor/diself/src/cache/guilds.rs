use crate::model::Guild;
use dashmap::DashMap;
use std::sync::Arc;

/// Cache for guilds (guild_id -> Guild)
#[derive(Clone)]
pub struct GuildCache {
    enabled: bool,
    guilds: Arc<DashMap<String, Guild>>,
}

impl GuildCache {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            guilds: Arc::new(DashMap::new()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn get(&self, guild_id: &str) -> Option<Guild> {
        self.guilds.get(guild_id).map(|entry| entry.clone())
    }

    pub fn insert(&self, guild: Guild) {
        if self.enabled {
            self.guilds.insert(guild.id.clone(), guild);
        }
    }

    pub fn remove(&self, guild_id: &str) -> Option<Guild> {
        self.guilds.remove(guild_id).map(|(_, guild)| guild)
    }

    pub fn count(&self) -> usize {
        self.guilds.len()
    }

    pub fn all(&self) -> Vec<Guild> {
        self.guilds
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn clear(&self) {
        self.guilds.clear();
    }

    /// Initializes the guild cache with data from the READY event
    pub fn initialize_from_ready(&self, data: serde_json::Value) {
        if let Some(guilds) = data.as_array() {
            for guild in guilds {
                match serde_json::from_value::<Guild>(guild.clone()) {
                    Ok(g) => {
                        self.insert(g.clone());
                    }
                    Err(e) => eprintln!(
                        "Failed to deserialize guild for guild cache initialization: {}",
                        e
                    ),
                }
            }
        } else {
            eprintln!("Expected guilds array in READY event data for guild cache initialization");
        }
    }
}
