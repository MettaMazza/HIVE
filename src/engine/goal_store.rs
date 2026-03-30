/// GoalStore — multi-scope cache for goal trees.
///
/// Extracted from goals.rs for module size management.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use super::goals::GoalTree;

pub struct GoalStore {
    trees: RwLock<HashMap<String, Arc<GoalTree>>>,
    project_root: String,
}

impl GoalStore {
    pub fn new(project_root: &str) -> Self {
        Self {
            trees: RwLock::new(HashMap::new()),
            project_root: project_root.to_string(),
        }
    }

    /// Get or create the goal tree for a scope.
    pub async fn get_tree(&self, scope: &crate::models::scope::Scope) -> Arc<GoalTree> {
        let key = scope.to_key();
        
        // Fast path: read lock
        {
            let trees = self.trees.read().await;
            if let Some(tree) = trees.get(&key) {
                return tree.clone();
            }
        }
        
        // Slow path: write lock, create tree
        let mut trees = self.trees.write().await;
        // Double-check after acquiring write lock
        if let Some(tree) = trees.get(&key) {
            return tree.clone();
        }
        
        let tree = Arc::new(GoalTree::new(&self.project_root, &key));
        trees.insert(key, tree.clone());
        tree
    }
}
