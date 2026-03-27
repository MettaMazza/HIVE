/// Global HUD — Shows the NeuroLease mesh status in Apis's system prompt.
///
/// This is injected into the system prompt ONLY — never surfaced in replies to users.
/// Users cannot see, query, or interact with the mesh HUD.
use crate::network::discovery::PeerRegistry;
use crate::network::trust::TrustStore;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Format the mesh status as a HUD block for injection into the system prompt.
pub async fn format_mesh_hud(
    registry: &PeerRegistry,
    trust_store: &Arc<RwLock<TrustStore>>,
    local_version: &str,
    lessons_shared: u64,
) -> String {
    let peers = registry.all_peers().await;
    let trust = trust_store.read().await;

    if peers.is_empty() {
        return String::from(
            "╔═══ NEUROLEASE MESH ═══════════════════════╗\n\
             ║ 🌐 No peers connected | Mesh active       ║\n\
             ╚═══════════════════════════════════════════╝"
        );
    }

    let mut lines = vec![
        format!("╔═══ NEUROLEASE MESH ═══════════════════════════════════╗"),
        format!("║ 🌐 {} peers online | {} lessons shared", peers.len(), lessons_shared),
    ];

    let now = chrono::Utc::now();
    for (i, peer) in peers.iter().enumerate() {
        let trust_level = trust.trust_level(&peer.peer_id);
        let ago = chrono::DateTime::parse_from_rfc3339(&peer.last_seen)
            .map(|dt| {
                let secs = (now - dt.with_timezone(&chrono::Utc)).num_seconds();
                if secs < 60 { format!("{}s ago", secs) }
                else if secs < 3600 { format!("{}m ago", secs / 60) }
                else { format!("{}h ago", secs / 3600) }
            })
            .unwrap_or_else(|_| "?".to_string());

        let prefix = if i == peers.len() - 1 { "└─" } else { "├─" };
        lines.push(format!(
            "║ {} apis@{} (v.{}) — {} — {} ",
            prefix, peer.peer_id, &peer.version[..7.min(peer.version.len())],
            ago, trust_level
        ));
    }

    lines.push(format!("║ 📡 Local: v.{} | Mesh protocol v1", &local_version[..7.min(local_version.len())]));
    lines.push(format!("╚═══════════════════════════════════════════════════════╝"));

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::messages::{PeerId, PeerInfo};

    #[tokio::test]
    async fn test_empty_mesh_hud() {
        let registry = PeerRegistry::new();
        let trust = Arc::new(RwLock::new(TrustStore::new(
            &std::env::temp_dir().join(format!("hive_hud_test_{}", std::process::id()))
        )));

        let hud = format_mesh_hud(&registry, &trust, "cafb023", 0).await;
        assert!(hud.contains("No peers connected"));
    }

    #[tokio::test]
    async fn test_populated_mesh_hud() {
        let registry = PeerRegistry::new();
        registry.upsert(PeerInfo {
            peer_id: PeerId("peer_alpha_key".into()),
            addr: "192.168.1.100:9473".into(),
            last_seen: chrono::Utc::now().to_rfc3339(),
            version: "cafb023e0".into(),
            binary_hash: "hash".into(),
            source_hash: "src".into(),
        }).await;

        let trust = Arc::new(RwLock::new(TrustStore::new(
            &std::env::temp_dir().join(format!("hive_hud_test2_{}", std::process::id()))
        )));

        let hud = format_mesh_hud(&registry, &trust, "cafb023e0", 47).await;
        assert!(hud.contains("1 peers online"));
        assert!(hud.contains("47 lessons shared"));
        assert!(hud.contains("peer_alpha_"));
    }
}
