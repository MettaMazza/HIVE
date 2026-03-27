/// Knowledge Sync — Lesson and synaptic graph synchronization across the mesh.
///
/// Handles ingestion of remote lessons and synaptic data, with:
/// - Deduplication by lesson ID
/// - PII sanitization (rejects data containing user identifiers)
/// - Confidence capping (all remote lessons capped at 0.8)
/// - CRDT-style merge for synaptic nodes (union of data entries)
/// - Staging inbox for review before merge
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::fs;
use crate::memory::lessons::Lesson;
use crate::memory::synaptic::{SynapticNode, SynapticEdge};
use crate::network::exporter::{sanitize_for_mesh, contains_pii};
use crate::network::trust::TrustLevel;

/// Manages the staging inbox for mesh-received knowledge.
pub struct KnowledgeSync {
    inbox_dir: PathBuf,
    /// Set of lesson IDs already ingested (prevents duplicates)
    seen_ids: HashSet<String>,
}

impl KnowledgeSync {
    pub fn new(mesh_dir: &std::path::Path) -> Self {
        let inbox_dir = mesh_dir.join("mesh_inbox");
        let _ = std::fs::create_dir_all(inbox_dir.join("lessons"));
        let _ = std::fs::create_dir_all(inbox_dir.join("synaptic"));
        let _ = std::fs::create_dir_all(inbox_dir.join("golden"));

        // Load previously seen IDs from the dedup log
        let seen_ids = Self::load_seen_ids(&inbox_dir);

        Self { inbox_dir, seen_ids }
    }

    fn load_seen_ids(inbox_dir: &std::path::Path) -> HashSet<String> {
        let dedup_path = inbox_dir.join("seen_ids.jsonl");
        if dedup_path.exists() {
            std::fs::read_to_string(&dedup_path)
                .unwrap_or_default()
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            HashSet::new()
        }
    }

    fn save_seen_id(&self, id: &str) {
        use std::io::Write;
        let dedup_path = self.inbox_dir.join("seen_ids.jsonl");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&dedup_path) {
            let _ = writeln!(f, "{}", id);
        }
    }

    /// Ingest a remote lesson. Returns Ok(true) if accepted, Ok(false) if rejected.
    pub fn ingest_lesson(&mut self, mut lesson: Lesson, peer_trust: TrustLevel) -> Result<bool, String> {
        // 1. Dedup by ID
        if self.seen_ids.contains(&lesson.id) {
            tracing::debug!("[SYNC] Duplicate lesson {} — skipping", lesson.id);
            return Ok(false);
        }

        // 2. PII scan — reject if user data detected
        if contains_pii(&lesson.text) {
            return Err(format!("PII detected in lesson text: {}", &lesson.id));
        }
        for kw in &lesson.keywords {
            if contains_pii(kw) {
                return Err(format!("PII detected in lesson keyword: {}", &lesson.id));
            }
        }

        // 3. Sanitize text (strip anything that slipped through)
        lesson.text = sanitize_for_mesh(&lesson.text);

        // 4. Cap confidence for all remote lessons (no single peer can inject 1.0)
        if lesson.confidence > 0.8 {
            lesson.confidence = 0.8;
        }

        // 5. Write to staging inbox
        let inbox_path = self.inbox_dir.join("lessons").join(format!("{}.json", lesson.id));
        if let Ok(json) = serde_json::to_string_pretty(&lesson) {
            if std::fs::write(&inbox_path, json).is_ok() {
                self.seen_ids.insert(lesson.id.clone());
                self.save_seen_id(&lesson.id);
                tracing::info!("[SYNC] 📥 Lesson staged: {} (confidence: {:.2}, trust: {})",
                    &lesson.id[..8], lesson.confidence, peer_trust);
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Ingest remote synaptic data. CRDT union merge into staging inbox.
    pub fn ingest_synaptic(
        &self,
        nodes: Vec<SynapticNode>,
        edges: Vec<SynapticEdge>,
        _peer_trust: TrustLevel,
    ) -> Result<usize, String> {
        let mut accepted = 0;

        // Validate and stage nodes
        for node in &nodes {
            // PII scan on concept and data entries
            if contains_pii(&node.concept) {
                return Err(format!("PII in synaptic concept: {}", node.concept));
            }
            for d in &node.data {
                if contains_pii(d) {
                    return Err(format!("PII in synaptic data for concept: {}", node.concept));
                }
            }
        }

        // Write delta to staging
        let delta_id = uuid::Uuid::new_v4().to_string();
        let delta = SynapticDelta { nodes, edges };
        let delta_path = self.inbox_dir.join("synaptic").join(format!("{}.json", delta_id));
        if let Ok(json) = serde_json::to_string_pretty(&delta) {
            if std::fs::write(&delta_path, json).is_ok() {
                accepted = delta.nodes.len() + delta.edges.len();
                tracing::info!("[SYNC] 📥 Synaptic delta staged: {} items", accepted);
            }
        }

        Ok(accepted)
    }

    /// Merge all staged lessons into the local LessonsManager.
    /// Called during autonomy sessions — Apis reviews before merging.
    pub async fn merge_staged_lessons(&self, lessons_mgr: &crate::memory::lessons::LessonsManager) -> usize {
        let lessons_dir = self.inbox_dir.join("lessons");
        let mut merged = 0;

        let entries = match fs::read_dir(&lessons_dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut entries = entries;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(content) = fs::read_to_string(&path).await {
                    if let Ok(lesson) = serde_json::from_str::<Lesson>(&content) {
                        // Merge into global scope (mesh lessons are scope-agnostic)
                        let global_scope = crate::models::scope::Scope::Private {
                            user_id: "mesh_global".to_string(),
                        };
                        if lessons_mgr.add_lesson(&global_scope, &lesson).await.is_ok() {
                            merged += 1;
                            let _ = fs::remove_file(&path).await; // Consumed
                        }
                    }
                }
            }
        }

        if merged > 0 {
            tracing::info!("[SYNC] ✅ Merged {} lessons from mesh inbox", merged);
        }
        merged
    }

    /// Merge all staged synaptic deltas into the local graph.
    pub async fn merge_staged_synaptic(&self, graph: &crate::memory::synaptic::Neo4jGraph) -> usize {
        let synaptic_dir = self.inbox_dir.join("synaptic");
        let mut merged = 0;

        let entries = match fs::read_dir(&synaptic_dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut entries = entries;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(content) = fs::read_to_string(&path).await {
                    if let Ok(delta) = serde_json::from_str::<SynapticDelta>(&content) {
                        // CRDT merge: store all nodes (dedup handled by Neo4jGraph)
                        for node in &delta.nodes {
                            for data in &node.data {
                                graph.store(&node.concept, data).await;
                                merged += 1;
                            }
                        }
                        // Store all edges (dedup handled by Neo4jGraph)
                        for edge in &delta.edges {
                            graph.store_relationship(&edge.from, &edge.relation, &edge.to).await;
                            merged += 1;
                        }
                        let _ = fs::remove_file(&path).await; // Consumed
                    }
                }
            }
        }

        if merged > 0 {
            tracing::info!("[SYNC] ✅ Merged {} synaptic entries from mesh inbox", merged);
        }
        merged
    }

    /// Purge all staged data (emergency cleanup).
    pub async fn purge_inbox(&self) {
        for subdir in &["lessons", "synaptic", "golden"] {
            let dir = self.inbox_dir.join(subdir);
            if dir.exists() {
                let _ = fs::remove_dir_all(&dir).await;
                let _ = fs::create_dir_all(&dir).await;
            }
        }
        tracing::warn!("[SYNC] 🗑️ Mesh inbox purged");
    }
}

/// Serializable synaptic delta for staging.
#[derive(Debug, Serialize, Deserialize)]
struct SynapticDelta {
    nodes: Vec<SynapticNode>,
    edges: Vec<SynapticEdge>,
}

use serde::{Serialize, Deserialize};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::lessons::Lesson;
    use uuid::Uuid;

    fn test_lesson(text: &str) -> Lesson {
        Lesson {
            id: Uuid::new_v4().to_string(),
            text: text.to_string(),
            keywords: vec!["test".into()],
            confidence: 0.8,
            origin: "peer_abc".to_string(),
            learned_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn test_dedup() {
        let tmp = std::env::temp_dir().join(format!("hive_sync_test_{}", std::process::id()));
        let mut sync = KnowledgeSync::new(&tmp);

        let lesson = test_lesson("Rust is fast");
        let id = lesson.id.clone();

        assert!(sync.ingest_lesson(lesson.clone(), TrustLevel::Attested).unwrap());
        assert!(!sync.ingest_lesson(lesson, TrustLevel::Attested).unwrap()); // Duplicate

        assert!(sync.seen_ids.contains(&id));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_pii_rejection() {
        let tmp = std::env::temp_dir().join(format!("hive_sync_pii_{}", std::process::id()));
        let mut sync = KnowledgeSync::new(&tmp);

        let pii_lesson = test_lesson("User 1299810741984956449 likes cats");
        let result = sync.ingest_lesson(pii_lesson, TrustLevel::Attested);
        assert!(result.is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_confidence_capping() {
        let tmp = std::env::temp_dir().join(format!("hive_sync_cap_{}", std::process::id()));
        let mut sync = KnowledgeSync::new(&tmp);

        let mut lesson = test_lesson("Sky is blue");
        lesson.confidence = 0.95;

        sync.ingest_lesson(lesson.clone(), TrustLevel::Attested).unwrap();

        // Read back the staged file to verify confidence was capped
        let staged_path = tmp.join("mesh_inbox/lessons").join(format!("{}.json", lesson.id));
        let content = std::fs::read_to_string(staged_path).unwrap();
        let staged: Lesson = serde_json::from_str(&content).unwrap();
        assert!(staged.confidence <= 0.8,
            "Remote lesson confidence capped to 0.8 (was: {})", staged.confidence);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_clean_lesson_accepted() {
        let tmp = std::env::temp_dir().join(format!("hive_sync_clean_{}", std::process::id()));
        let mut sync = KnowledgeSync::new(&tmp);

        let lesson = test_lesson("Memory management in Rust uses ownership");
        assert!(sync.ingest_lesson(lesson, TrustLevel::Attested).unwrap());

        std::fs::remove_dir_all(&tmp).ok();
    }
}
