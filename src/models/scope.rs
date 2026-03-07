use serde::{Deserialize, Serialize};

/// Defines the security scope of an event or memory context.
/// 
/// A primary tenet of the HIVE system is strict data segregation.
/// - `Public`: Broadly accessible data (e.g. general Discord Channels).
/// - `Private`: Data tied exclusively to a specific User's identity (e.g. Direct Messages).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    Public,
    /// A secure 1-to-1 context for a specific user ID.
    Private {
        user_id: String,
    },
}

impl Scope {
    /// Determines if a process operating under `self`'s scope is permitted
    /// to read data that is tagged with the `target` scope.
    ///
    /// Rules:
    /// - `Public` scope can ONLY read `Public` data.
    /// - `Private(X)` scope can read `Public` data AND `Private(X)` data.
    /// - `Private(X)` CANNOT read `Private(Y)` data.
    pub fn can_read(&self, target: &Scope) -> bool {
        match (self, target) {
            // Anyone can read public data
            (_, Scope::Public) => true,
            // Public scope cannot read private data
            (Scope::Public, Scope::Private { .. }) => false,
            // Private scope can only read its own private data
            (Scope::Private { user_id: req_id }, Scope::Private { user_id: target_id }) => {
                req_id == target_id
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_visibility() {
        let public = Scope::Public;
        let priv_alice = Scope::Private { user_id: "alice".to_string() };
        let priv_bob = Scope::Private { user_id: "bob".to_string() };

        // Public can read Public
        assert!(public.can_read(&public));
        
        // Public CANNOT read Private
        assert!(!public.can_read(&priv_alice));

        // Private can read Public
        assert!(priv_alice.can_read(&public));

        // Private can read own Private
        assert!(priv_alice.can_read(&priv_alice));

        // Private CANNOT read other's Private
        assert!(!priv_alice.can_read(&priv_bob));
    }
}
