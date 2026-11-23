use crate::model::{Relationship, RelationshipType};
use dashmap::DashMap;
use std::sync::Arc;

/// Cache for relationships (user_id -> Relationship)
#[derive(Clone)]
pub struct RelationshipCache {
    enabled: bool,
    relationships: Arc<DashMap<String, Relationship>>,
}

impl RelationshipCache {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            relationships: Arc::new(DashMap::new()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn get(&self, user_id: &str) -> Option<Relationship> {
        self.relationships.get(user_id).map(|entry| entry.clone())
    }

    pub fn insert(&self, relationship: Relationship) {
        if self.enabled {
            self.relationships
                .insert(relationship.id.clone(), relationship);
        }
    }

    pub fn remove(&self, user_id: &str) -> Option<Relationship> {
        self.relationships.remove(user_id).map(|(_, rel)| rel)
    }

    pub fn count(&self) -> usize {
        self.relationships.len()
    }

    pub fn all(&self) -> Vec<Relationship> {
        self.relationships
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn clear(&self) {
        self.relationships.clear();
    }

    /// Initializes the relationship cache with data from the READY event
    pub fn initialize_from_ready(&self, data: serde_json::Value) {
        if let Some(relationships) = data.as_array() {
            for relationship in relationships {
                match serde_json::from_value::<Relationship>(relationship.clone()) {
                    Ok(r) => self.insert(r),
                    Err(e) => eprintln!("Failed to deserialize relationship for relationship cache initialization: {}", e),
                }
            }
        } else {
            eprintln!(
                "Expected an array of relationships for relationship cache initialization, but got: {}",
                data
            );
        }
    }

    pub fn friends(&self) -> Vec<Relationship> {
        self.relationships
            .iter()
            .filter(|entry| entry.value().kind == RelationshipType::Friend)
            .map(|entry| entry.value().clone())
            .collect()
    }
}
