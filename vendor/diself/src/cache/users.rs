use crate::model::User;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct UserCache {
    enabled: bool,
    users: Arc<DashMap<String, User>>,
}

impl UserCache {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            users: Arc::new(DashMap::new()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn get(&self, user_id: &str) -> Option<User> {
        self.users.get(user_id).map(|entry| entry.clone())
    }

    pub fn insert(&self, user: User) {
        if self.enabled {
            self.users.insert(user.id.clone(), user);
        }
    }

    pub fn remove(&self, user_id: &str) -> Option<User> {
        self.users.remove(user_id).map(|(_, user)| user)
    }

    pub fn count(&self) -> usize {
        self.users.len()
    }

    pub fn all(&self) -> Vec<User> {
        self.users
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn clear(&self) {
        self.users.clear();
    }

    pub fn initialize_from_ready(&self, data: serde_json::Value) {
        if let Some(users) = data.as_array() {
            for user in users {
                match serde_json::from_value::<User>(user.clone()) {
                    Ok(u) => self.insert(u),
                    Err(e) => eprintln!(
                        "Failed to deserialize user for user cache initialization: {}",
                        e
                    ),
                }
            }
        } else {
            eprintln!(
                "Expected an array of users for user cache initialization, but got: {}",
                data
            );
        }
    }
}

impl Default for UserCache {
    fn default() -> Self {
        Self::new(true)
    }
}
