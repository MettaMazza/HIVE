use std::sync::Arc;
use tokio::sync::RwLock;
use crate::models::message::Event;
use crate::models::scope::Scope;

/// In-memory storage of interactions.
/// Will eventually bridge to a SQLite or Postgres database per your preference.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    events: Arc<RwLock<Vec<Event>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Stores a new event into memory.
    pub async fn add_event(&self, event: Event) {
        let mut events = self.events.write().await;
        events.push(event);
    }

    /// Retrieves history strictly filtered by what the requesting `Scope` is allowed to see.
    /// This is the core of HIVE's security model.
    pub async fn get_history(&self, requesting_scope: &Scope) -> Vec<Event> {
        let events = self.events.read().await;
        
        events
            .iter()
            .filter(|e| requesting_scope.can_read(&e.scope))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_secure_memory_retrieval() {
        let store = MemoryStore::new();

        let public_event = Event {
            platform: "discord".to_string(),
            scope: Scope::Public,
            author_name: "Alice".to_string(),
            content: "Hello everyone!".to_string(),
        };

        let private_alice_event = Event {
            platform: "discord".to_string(),
            scope: Scope::Private { user_id: "alice_id".to_string() },
            author_name: "Alice".to_string(),
            content: "My secret diary.".to_string(),
        };

        let private_bob_event = Event {
            platform: "discord".to_string(),
            scope: Scope::Private { user_id: "bob_id".to_string() },
            author_name: "Bob".to_string(),
            content: "Bank PIN: 1234".to_string(),
        };

        store.add_event(public_event).await;
        store.add_event(private_alice_event).await;
        store.add_event(private_bob_event).await;

        // Public Scope Query -> Should only see Public event
        let public_history = store.get_history(&Scope::Public).await;
        assert_eq!(public_history.len(), 1);
        assert_eq!(public_history[0].scope, Scope::Public);

        // Alice Private Scope Query -> Should see Public + Alice Private
        let alice_scope = Scope::Private { user_id: "alice_id".to_string() };
        let alice_history = store.get_history(&alice_scope).await;
        assert_eq!(alice_history.len(), 2);
        let has_bob = alice_history.iter().any(|e| matches!(&e.scope, Scope::Private { user_id } if user_id == "bob_id"));
        assert!(!has_bob, "Alice saw Bob's private data! Security breach!");

        // Bob Private Scope Query -> Should see Public + Bob Private
        let bob_scope = Scope::Private { user_id: "bob_id".to_string() };
        let bob_history = store.get_history(&bob_scope).await;
        assert_eq!(bob_history.len(), 2);
        let has_alice = bob_history.iter().any(|e| matches!(&e.scope, Scope::Private { user_id } if user_id == "alice_id"));
        assert!(!has_alice, "Bob saw Alice's private data! Security breach!");
    }
}
