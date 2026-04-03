use color_eyre::eyre::{Result, WrapErr};
use rusqlite::Connection;

pub const KEY_LAST_GUILD: &str = "last_guild_id";
pub const KEY_LAST_CHANNEL: &str = "last_channel_id";

/// Persistent state stored in `SQLite` at the platform cache directory.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .or_else(|| {
                tracing::warn!("no platform cache dir found, using current directory for state db");
                None
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("disctui");
        std::fs::create_dir_all(&cache_dir)?;

        let db_path = cache_dir.join("disctui.db");
        tracing::info!("state db: {}", db_path.display());

        let conn = Connection::open(&db_path).wrap_err("failed to open state database")?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS read_state (
                channel_id TEXT PRIMARY KEY,
                last_message_id TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            CREATE TABLE IF NOT EXISTS session (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .wrap_err("failed to initialize database schema")?;

        Ok(Self { conn })
    }

    /// Get the last-read message ID for a channel.
    pub fn last_read_message(&self, channel_id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT last_message_id FROM read_state WHERE channel_id = ?1",
                [channel_id],
                |row| row.get(0),
            )
            .ok()
    }

    /// Mark a channel as read up to the given message ID.
    pub fn mark_read(&self, channel_id: &str, message_id: &str) {
        let result = self.conn.execute(
            "INSERT INTO read_state (channel_id, last_message_id, updated_at)
             VALUES (?1, ?2, strftime('%s','now'))
             ON CONFLICT(channel_id) DO UPDATE SET
                last_message_id = excluded.last_message_id,
                updated_at = excluded.updated_at",
            [channel_id, message_id],
        );
        if let Err(e) = result {
            tracing::warn!("failed to update read state: {e}");
        }
    }

    /// Save a session key-value (e.g., `last_guild_id`, `last_channel_id`).
    pub fn set_session(&self, key: &str, value: &str) {
        let result = self.conn.execute(
            "INSERT INTO session (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        );
        if let Err(e) = result {
            tracing::warn!("failed to save session state: {e}");
        }
    }

    /// Get a session value.
    pub fn get_session(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM session WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .ok()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use super::*;

    fn test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS read_state (
                channel_id TEXT PRIMARY KEY,
                last_message_id TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            CREATE TABLE IF NOT EXISTS session (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .unwrap();
        Store { conn }
    }

    #[test]
    fn session_round_trip() {
        let store = test_store();
        assert!(store.get_session("foo").is_none());
        store.set_session("foo", "bar");
        assert_eq!(store.get_session("foo").unwrap(), "bar");
        store.set_session("foo", "baz");
        assert_eq!(store.get_session("foo").unwrap(), "baz");
    }

    #[test]
    fn mark_read_round_trip() {
        let store = test_store();
        assert!(store.last_read_message("ch1").is_none());
        store.mark_read("ch1", "100");
        assert_eq!(store.last_read_message("ch1").unwrap(), "100");
        store.mark_read("ch1", "200");
        assert_eq!(store.last_read_message("ch1").unwrap(), "200");
    }
}
