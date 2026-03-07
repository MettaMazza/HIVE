use serde::{Deserialize, Serialize};

use super::scope::Scope;

/// Represents a standardized Event/Message coming INTO HIVE from any Platform.
/// The `Scope` attaches strict security context to what triggered the event,
/// preventing private platform interactions from accessing public data or vice-versa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub platform: String,
    pub scope: Scope,
    pub author_name: String,
    pub content: String,
    // (Optional) ID for threads/channels if necessary, can add later
}

/// Represents a standardized Response going OUT of HIVE to a Platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub platform: String,
    pub target_scope: Scope,
    pub text: String,
    /// If true, this is a live telemetry update (edit the existing embed).
    /// If false, this is a final response (send as a new message).
    pub is_telemetry: bool,
}
