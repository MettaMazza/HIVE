/// Tier 3: Synaptic Memory — Local Knowledge Graph
///
/// A persistent, in-memory knowledge graph backed by JSONL files.  
/// Stores concepts (nodes) and their associated data entries, supporting  
/// fuzzy search, beliefs retrieval, and relationship tracking.
///
/// No external database required — all state is persisted as JSON Lines  
/// under `memory/synaptic/`.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

/// A single knowledge node: a concept with associated data entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynapticNode {
    pub concept: String,
    pub data: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    /// PeerId of the instance that created this node. "local" for locally-created.
    #[serde(default = "default_synaptic_origin")]
    pub origin: String,
}

/// A relationship between two concepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynapticEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub created_at: String,
    /// PeerId of the instance that created this edge. "local" for locally-created.
    #[serde(default = "default_synaptic_origin")]
    pub origin: String,
}

fn default_synaptic_origin() -> String { "local".to_string() }

#[derive(Debug)]
pub struct Neo4jGraph {
    /// In-memory graph: concept_name (lowercased) → node
    pub(crate) nodes: RwLock<HashMap<String, SynapticNode>>,
    /// Edges between concepts
    pub(crate) edges: RwLock<Vec<SynapticEdge>>,
    /// Directory for persistence
    dir: Option<PathBuf>,
}

impl Clone for Neo4jGraph {
    fn clone(&self) -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            dir: self.dir.clone(),
        }
    }
}

impl Default for Neo4jGraph {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Neo4jGraph {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let dir = base_dir.map(|d| {
            let p = d.join("synaptic");
            let _ = std::fs::create_dir_all(&p);
            p
        });
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            dir,
        }
    }

    /// Load persisted nodes and edges from disk.
    pub async fn load(&self) {
        if let Some(ref dir) = self.dir {
            // Load nodes
            let nodes_path = dir.join("nodes.jsonl");
            if nodes_path.exists()
                && let Ok(content) = tokio::fs::read_to_string(&nodes_path).await {
                    let mut map = self.nodes.write().await;
                    for line in content.lines() {
                        if let Ok(node) = serde_json::from_str::<SynapticNode>(line) {
                            map.insert(node.concept.to_lowercase(), node);
                        }
                    }
                    tracing::info!("[SYNAPTIC] Loaded {} nodes from disk.", map.len());
                }

            // Load edges
            let edges_path = dir.join("edges.jsonl");
            if edges_path.exists()
                && let Ok(content) = tokio::fs::read_to_string(&edges_path).await {
                    let mut edges = self.edges.write().await;
                    for line in content.lines() {
                        if let Ok(edge) = serde_json::from_str::<SynapticEdge>(line) {
                            edges.push(edge);
                        }
                    }
                    tracing::info!("[SYNAPTIC] Loaded {} edges from disk.", edges.len());
                }
        }
    }

    /// Persist all nodes to disk (full rewrite).
    async fn save_nodes(&self) {
        if let Some(ref dir) = self.dir {
            let nodes = self.nodes.read().await;
            let mut lines = Vec::with_capacity(nodes.len());
            for node in nodes.values() {
                if let Ok(json) = serde_json::to_string(node) {
                    lines.push(json);
                }
            }
            let content = lines.join("\n");
            if !content.is_empty() {
                let _ = tokio::fs::write(dir.join("nodes.jsonl"), content + "\n").await;
            }
        }
    }

    /// Persist all edges to disk (full rewrite).
    async fn save_edges(&self) {
        if let Some(ref dir) = self.dir {
            let edges = self.edges.read().await;
            let mut lines = Vec::with_capacity(edges.len());
            for edge in edges.iter() {
                if let Ok(json) = serde_json::to_string(edge) {
                    lines.push(json);
                }
            }
            let content = lines.join("\n");
            if !content.is_empty() {
                let _ = tokio::fs::write(dir.join("edges.jsonl"), content + "\n").await;
            }
        }
    }

    // ─── PUBLIC API ───────────────────────────────────────────────────

    /// Store a concept → data entry. If the concept already exists, the data
    /// entry is appended (not replaced). This builds up a multi-faceted
    /// understanding of each concept over time.
    pub async fn store(&self, concept: &str, data: &str) {
        let key = concept.to_lowercase();
        let now = chrono::Utc::now().to_rfc3339();
        {
            let mut nodes = self.nodes.write().await;
            let entry = nodes.entry(key.clone()).or_insert_with(|| SynapticNode {
                concept: concept.to_string(),
                data: Vec::new(),
                created_at: now.clone(),
                updated_at: now.clone(),
                origin: "local".to_string(),
            });
            // Avoid storing exact duplicates
            if !entry.data.iter().any(|d| d == data) {
                entry.data.push(data.to_string());
                entry.updated_at = now;
            }
        }
        self.save_nodes().await;
        tracing::info!("[SYNAPTIC] Stored: '{}' → '{}'", concept, data);
    }

    /// Search for a concept. Returns all data entries associated with it.
    /// Uses case-insensitive prefix matching so "appl" matches "apple".
    pub async fn search(&self, concept: &str) -> Vec<String> {
        let query = concept.to_lowercase();
        let nodes = self.nodes.read().await;

        // Exact match first
        if let Some(node) = nodes.get(&query) {
            return node.data.iter()
                .map(|d| format!("[{}] {}", node.concept, d))
                .collect();
        }

        // Fuzzy: prefix + substring match
        let mut results = Vec::new();
        for node in nodes.values() {
            let key = node.concept.to_lowercase();
            if key.starts_with(&query) || key.contains(&query) || query.contains(&key) {
                for d in &node.data {
                    results.push(format!("[{}] {}", node.concept, d));
                }
            }
        }
        results
    }

    /// Retrieve the most recently updated nodes.
    pub async fn get_recent_nodes(&self, limit: usize) -> Vec<(String, String)> {
        let nodes = self.nodes.read().await;
        let mut sorted: Vec<&SynapticNode> = nodes.values().collect();
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sorted.into_iter()
            .take(limit)
            .map(|n| (n.concept.clone(), n.data.join("; ")))
            .collect()
    }

    /// Retrieve core beliefs — the concepts with the most data entries,
    /// indicating the concepts Apis has stored the most knowledge about.
    pub async fn get_beliefs(&self, limit: usize) -> Vec<String> {
        let nodes = self.nodes.read().await;
        let mut sorted: Vec<&SynapticNode> = nodes.values().collect();
        // Sort by number of data entries (most knowledge = strongest beliefs)
        sorted.sort_by(|a, b| b.data.len().cmp(&a.data.len()));
        sorted.into_iter()
            .take(limit)
            .map(|n| {
                let summary = if n.data.len() <= 3 {
                    n.data.join("; ")
                } else {
                    format!("{} (+{} more)", n.data[..3].join("; "), n.data.len() - 3)
                };
                format!("{}: {}", n.concept, summary)
            })
            .collect()
    }

    /// Retrieve recent edges/relationships.
    pub async fn get_recent_relationships(&self, limit: usize) -> Vec<(String, String, String)> {
        let edges = self.edges.read().await;
        edges.iter()
            .rev()
            .take(limit)
            .map(|e| (e.from.clone(), e.relation.clone(), e.to.clone()))
            .collect()
    }

    /// Store a relationship between two concepts.
    pub async fn store_relationship(&self, from: &str, relation: &str, to: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        {
            let mut edges = self.edges.write().await;
            // Avoid exact duplicate edges
            let already = edges.iter().any(|e| {
                e.from.to_lowercase() == from.to_lowercase()
                    && e.to.to_lowercase() == to.to_lowercase()
                    && e.relation.to_lowercase() == relation.to_lowercase()
            });
            if !already {
                edges.push(SynapticEdge {
                    from: from.to_string(),
                    to: to.to_string(),
                    relation: relation.to_string(),
                    created_at: now,
                    origin: "local".to_string(),
                });
            }
        }
        self.save_edges().await;
        tracing::info!("[SYNAPTIC] Edge: '{}' --[{}]--> '{}'", from, relation, to);
    }

    /// Get total counts for diagnostics.
    pub async fn stats(&self) -> (usize, usize) {
        let nodes = self.nodes.read().await;
        let edges = self.edges.read().await;
        (nodes.len(), edges.len())
    }

    /// Export the entire raw graph for Visualizer Dashboard rendering.
    pub async fn export_graph(&self) -> (Vec<SynapticNode>, Vec<SynapticEdge>) {
        let nodes = self.nodes.read().await;
        let edges = self.edges.read().await;
        let n: Vec<SynapticNode> = nodes.values().cloned().collect();
        let e: Vec<SynapticEdge> = edges.clone();
        (n, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_search() {
        let graph = Neo4jGraph::new(None);

        // Store some concepts
        graph.store("Apple", "A red fruit").await;
        graph.store("Apple", "Grows on trees").await;
        graph.store("Banana", "A yellow fruit").await;

        // Exact search
        let results = graph.search("Apple").await;
        assert_eq!(results.len(), 2);
        assert!(results[0].contains("A red fruit"));
        assert!(results[1].contains("Grows on trees"));

        // Prefix/substring search
        let results = graph.search("app").await;
        assert_eq!(results.len(), 2);

        // No results
        let results = graph.search("Watermelon").await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_deduplicate_store() {
        let graph = Neo4jGraph::new(None);
        graph.store("Apple", "A red fruit").await;
        graph.store("Apple", "A red fruit").await; // duplicate
        let results = graph.search("Apple").await;
        assert_eq!(results.len(), 1); // Only stored once
    }

    #[tokio::test]
    async fn test_beliefs() {
        let graph = Neo4jGraph::new(None);
        graph.store("Apple", "fact 1").await;
        graph.store("Apple", "fact 2").await;
        graph.store("Apple", "fact 3").await;
        graph.store("Banana", "fact A").await;

        let beliefs = graph.get_beliefs(10).await;
        assert_eq!(beliefs.len(), 2);
        // Apple first (3 entries vs Banana's 1)
        assert!(beliefs[0].starts_with("Apple"));
        assert!(beliefs[1].starts_with("Banana"));
    }

    #[tokio::test]
    async fn test_recent_nodes() {
        let graph = Neo4jGraph::new(None);
        graph.store("First", "data").await;
        graph.store("Second", "data").await;

        let recent = graph.get_recent_nodes(1).await;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].0, "Second"); // Most recently updated
    }

    #[tokio::test]
    async fn test_relationships() {
        let graph = Neo4jGraph::new(None);
        graph.store_relationship("Apple", "is_a", "Fruit").await;
        graph.store_relationship("Banana", "is_a", "Fruit").await;

        let rels = graph.get_recent_relationships(10).await;
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].0, "Banana"); // Most recent first (reversed)
        assert_eq!(rels[0].1, "is_a");
        assert_eq!(rels[0].2, "Fruit");
    }

    #[tokio::test]
    async fn test_deduplicate_edges() {
        let graph = Neo4jGraph::new(None);
        graph.store_relationship("Apple", "is_a", "Fruit").await;
        graph.store_relationship("Apple", "is_a", "Fruit").await; // duplicate
        let rels = graph.get_recent_relationships(10).await;
        assert_eq!(rels.len(), 1); // Only stored once
    }

    #[tokio::test]
    async fn test_stats() {
        let graph = Neo4jGraph::new(None);
        graph.store("A", "1").await;
        graph.store("B", "2").await;
        graph.store_relationship("A", "related", "B").await;
        let (nodes, edges) = graph.stats().await;
        assert_eq!(nodes, 2);
        assert_eq!(edges, 1);
    }

    #[tokio::test]
    async fn test_persistence_with_tempdir() {
        let tmp = std::env::temp_dir().join(format!("hive_synaptic_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        // Write
        {
            let graph = Neo4jGraph::new(Some(tmp.clone()));
            graph.store("Maria", "is the creator of HIVE").await;
            graph.store("Maria", "believes in transparent AI").await;
            graph.store_relationship("Maria", "created", "HIVE").await;
        }

        // Read in a new instance
        {
            let graph = Neo4jGraph::new(Some(tmp.clone()));
            graph.load().await;

            let results = graph.search("Maria").await;
            assert_eq!(results.len(), 2);
            assert!(results[0].contains("creator of HIVE"));

            let rels = graph.get_recent_relationships(10).await;
            assert_eq!(rels.len(), 1);
            assert_eq!(rels[0].0, "Maria");
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
