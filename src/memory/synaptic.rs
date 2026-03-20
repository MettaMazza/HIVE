use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct Neo4jGraph {
    concepts: RwLock<HashMap<String, String>>,
    relationships: RwLock<Vec<(String, String, String)>>,
}

impl Default for Neo4jGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl Neo4jGraph {
    pub fn new() -> Self {
        Self {
            concepts: RwLock::new(HashMap::new()),
            relationships: RwLock::new(Vec::new()),
        }
    }

    pub async fn store(&self, concept: &str, data: &str) {
        self.concepts.write().await.insert(concept.to_string(), data.to_string());
    }

    pub async fn search(&self, concept: &str) -> Vec<String> {
        let concepts = self.concepts.read().await;
        if let Some(data) = concepts.get(concept) {
            vec![data.clone()]
        } else {
            vec![]
        }
    }

    pub async fn get_recent_nodes(&self, limit: usize) -> Vec<(String, String)> {
        let concepts = self.concepts.read().await;
        concepts.iter()
            .take(limit)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub async fn get_beliefs(&self, limit: usize) -> Vec<String> {
        let concepts = self.concepts.read().await;
        concepts.keys()
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn get_recent_relationships(&self, limit: usize) -> Vec<(String, String, String)> {
        let rels = self.relationships.read().await;
        rels.iter()
            .rev()
            .take(limit)
            .map(|(a, b, c)| (a.clone(), b.clone(), c.clone()))
            .collect()
    }

    pub async fn store_relationship(&self, from: &str, relation: &str, to: &str) {
        self.relationships.write().await.push((from.to_string(), relation.to_string(), to.to_string()));
    }

    pub async fn stats(&self) -> (usize, usize) {
        let concepts = self.concepts.read().await;
        let relationships = self.relationships.read().await;
        (concepts.len(), relationships.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_synaptic_stubs() {
        let syn = Neo4jGraph::new();
        let syn2 = Neo4jGraph::default();

        syn.store("concept1", "data1").await;
        syn.store("concept2", "data2").await;
        syn.store_relationship("A", "relates_to", "B").await;
        syn.store_relationship("B", "relates_to", "C").await;

        assert_eq!(syn.search("concept1").await, vec!["data1"]);
        assert_eq!(syn.search("nonexistent").await, vec![] as Vec<String>);

        let nodes = syn.get_recent_nodes(5).await;
        assert_eq!(nodes.len(), 2);

        let beliefs = syn.get_beliefs(5).await;
        assert_eq!(beliefs.len(), 2);

        let rels = syn.get_recent_relationships(5).await;
        assert_eq!(rels.len(), 2);

        let (num_concepts, num_rels) = syn.stats().await;
        assert_eq!(num_concepts, 2);
        assert_eq!(num_rels, 2);

        let _ = syn2;
    }
}
