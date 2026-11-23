/// Configuration for the global cache behavior.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Whether to cache users.
    pub cache_users: bool,
    /// Whether to cache channels.
    pub cache_channels: bool,
    /// Whether to cache guilds.
    pub cache_guilds: bool,
    /// Whether to cache relationships.
    pub cache_relationships: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_users: true,
            cache_channels: true,
            cache_guilds: true,
            cache_relationships: true,
        }
    }
}
