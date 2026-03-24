use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ─── Goal Node ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Pending,
    Active,
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalSource {
    User,
    Autonomy,
    Decomposition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalNode {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
    pub priority: f64,           // 0.0–1.0
    pub depth: u8,               // 0 = root, 1 = subgoal, etc.
    pub parent_id: Option<String>,
    pub children: Vec<String>,   // child goal IDs

    // Progress
    pub progress: f64,           // 0.0–1.0
    pub evidence: Vec<String>,   // observations proving progress

    // Temporal
    pub created_at: f64,
    pub updated_at: f64,
    pub deadline: Option<f64>,

    // Metadata
    pub tags: Vec<String>,
    pub source: GoalSource,
    pub dependencies: Vec<String>,
}

impl GoalNode {
    pub fn new(title: String, description: String, priority: f64, source: GoalSource) -> Self {
        let now = now_ts();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            description,
            status: GoalStatus::Pending,
            priority: priority.clamp(0.0, 1.0),
            depth: 0,
            parent_id: None,
            children: vec![],
            progress: 0.0,
            evidence: vec![],
            created_at: now,
            updated_at: now,
            deadline: None,
            tags: vec![],
            source,
            dependencies: vec![],
        }
    }
}

// ─── Goal Tree ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct GoalTreeData {
    pub nodes: Vec<GoalNode>,
}


/// Persistent goal tree scoped to a user+location pair.
pub struct GoalTree {
    data: Mutex<GoalTreeData>,
    persist_path: PathBuf,
}

impl GoalTree {
    /// Load or create a goal tree for the given scope key.
    pub fn new(project_root: &str, scope_key: &str) -> Self {
        let path = PathBuf::from(project_root)
            .join("memory/core/goals")
            .join(format!("{}.json", scope_key));
        let data = Self::load(&path);
        Self {
            data: Mutex::new(data),
            persist_path: path,
        }
    }

    fn load(path: &PathBuf) -> GoalTreeData {
        if path.exists() {
            if let Ok(raw) = tokio::task::block_in_place(|| std::fs::read_to_string(path)) {
                if let Ok(data) = serde_json::from_str::<GoalTreeData>(&raw) {
                    return data;
                }
            }
        }
        GoalTreeData::default()
    }

    fn save(data: &GoalTreeData, path: &PathBuf) {
        if let Some(parent) = path.parent() {
            let _ = tokio::task::block_in_place(|| std::fs::create_dir_all(parent));
        }
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = tokio::task::block_in_place(|| std::fs::write(path, json));
        }
    }

    // ─── CRUD Operations ───────────────────────────────────────────────

    /// Add a new root goal. Returns the goal ID.
    pub async fn add_root_goal(&self, title: String, description: String, priority: f64, source: GoalSource, tags: Vec<String>) -> String {
        let mut data = self.data.lock().await;
        let mut node = GoalNode::new(title, description, priority, source);
        node.tags = tags;
        node.status = GoalStatus::Active;
        let id = node.id.clone();
        data.nodes.push(node);
        Self::save(&data, &self.persist_path);
        id
    }

    /// Add a subgoal under a parent. Returns the subgoal ID, or None if parent not found.
    pub async fn add_subgoal(&self, parent_id: &str, title: String, description: String, priority: f64, tags: Vec<String>) -> Option<String> {
        let mut data = self.data.lock().await;
        let parent_depth = data.nodes.iter().find(|n| n.id == parent_id).map(|n| n.depth)?;
        
        let mut node = GoalNode::new(title, description, priority, GoalSource::Decomposition);
        node.parent_id = Some(parent_id.to_string());
        node.depth = parent_depth + 1;
        node.tags = tags;
        node.status = GoalStatus::Pending;
        let id = node.id.clone();
        
        // Register child on parent
        if let Some(parent) = data.nodes.iter_mut().find(|n| n.id == parent_id) {
            parent.children.push(id.clone());
            parent.updated_at = now_ts();
        }
        
        data.nodes.push(node);
        Self::save(&data, &self.persist_path);
        Some(id)
    }

    /// Get a goal by ID.
    pub async fn get_goal(&self, id: &str) -> Option<GoalNode> {
        let data = self.data.lock().await;
        data.nodes.iter().find(|n| n.id == id).cloned()
    }

    /// Update a goal's status. Triggers progress recalculation on parent.
    pub async fn update_status(&self, id: &str, status: GoalStatus) -> bool {
        self.update_status_safe(id, status).await.unwrap_or(false)
    }

    /// Safely update status returning an error if blocked by graph dependencies.
    pub async fn update_status_safe(&self, id: &str, status: GoalStatus) -> Result<bool, String> {
        let mut data = self.data.lock().await;

        // Dependency Check
        if status == GoalStatus::Active || status == GoalStatus::Completed {
            let mut blocked_by = Vec::new();
            if let Some(node) = data.nodes.iter().find(|n| n.id == id) {
                for dep_id in &node.dependencies {
                    if let Some(dep) = data.nodes.iter().find(|n| n.id == *dep_id) {
                        if dep.status != GoalStatus::Completed {
                            blocked_by.push(dep.title.clone());
                        }
                    }
                }
            }
            if !blocked_by.is_empty() {
                return Err(format!("Cannot activate goal. Blocked by incomplete dependencies: {}", blocked_by.join(", ")));
            }
        }

        let (parent_id, found) = {
            if let Some(node) = data.nodes.iter_mut().find(|n| n.id == id) {
                if status == GoalStatus::Completed {
                    node.progress = 1.0;
                }
                node.status = status;
                node.updated_at = now_ts();
                (node.parent_id.clone(), true)
            } else {
                (None, false)
            }
        };
        if found {
            // Recalculate parent progress
            if let Some(pid) = parent_id {
                Self::recalc_progress(&mut data, &pid);
            }
            Self::save(&data, &self.persist_path);
        }
        Ok(found)
    }

    /// Manage goal dependency arrays natively
    pub async fn set_dependencies(&self, id: &str, deps: Vec<String>) {
        let mut data = self.data.lock().await;
        if let Some(node) = data.nodes.iter_mut().find(|n| n.id == id) {
            node.dependencies = deps;
            node.updated_at = now_ts();
        }
        Self::save(&data, &self.persist_path);
    }

    /// Add evidence of progress to a goal.
    pub async fn add_evidence(&self, id: &str, evidence: String, progress_delta: f64) -> bool {
        let mut data = self.data.lock().await;
        let parent_id = {
            if let Some(node) = data.nodes.iter_mut().find(|n| n.id == id) {
                node.evidence.push(evidence);
                node.progress = (node.progress + progress_delta).clamp(0.0, 1.0);
                node.updated_at = now_ts();
                if node.progress >= 1.0 {
                    node.status = GoalStatus::Completed;
                }
                node.parent_id.clone()
            } else {
                return false;
            }
        };
        if let Some(pid) = parent_id {
            Self::recalc_progress(&mut data, &pid);
        }
        Self::save(&data, &self.persist_path);
        true
    }

    // ─── Tree Queries ──────────────────────────────────────────────────

    /// Get all active root goals (depth 0, not completed/failed).
    pub async fn get_active_roots(&self) -> Vec<GoalNode> {
        let data = self.data.lock().await;
        data.nodes.iter()
            .filter(|n| n.depth == 0 && matches!(n.status, GoalStatus::Active | GoalStatus::Pending))
            .cloned()
            .collect()
    }

    /// Get all goals (for full tree view).
    pub async fn get_all(&self) -> Vec<GoalNode> {
        let data = self.data.lock().await;
        data.nodes.clone()
    }

    /// Get the deepest incomplete leaf goals — these are actionable.
    pub async fn get_actionable(&self) -> Vec<GoalNode> {
        let data = self.data.lock().await;
        data.nodes.iter()
            .filter(|n| {
                n.children.is_empty()
                    && matches!(n.status, GoalStatus::Active | GoalStatus::Pending)
            })
            .cloned()
            .collect()
    }

    /// Count total and completed goals.
    pub async fn stats(&self) -> (usize, usize) {
        let data = self.data.lock().await;
        let total = data.nodes.len();
        let completed = data.nodes.iter().filter(|n| n.status == GoalStatus::Completed).count();
        (total, completed)
    }

    /// Archive completed root subtrees (remove from active tree).
    pub async fn prune_completed(&self) -> usize {
        let mut data = self.data.lock().await;
        let completed_roots: Vec<String> = data.nodes.iter()
            .filter(|n| n.depth == 0 && n.status == GoalStatus::Completed)
            .map(|n| n.id.clone())
            .collect();

        let mut pruned = 0;
        for root_id in &completed_roots {
            let subtree_ids = Self::collect_subtree_ids(&data, root_id);
            data.nodes.retain(|n| !subtree_ids.contains(&n.id));
            pruned += subtree_ids.len();
        }

        if pruned > 0 {
            Self::save(&data, &self.persist_path);
        }
        pruned
    }

    // ─── Formatting ────────────────────────────────────────────────────

    /// Format the goal tree for injection into the HUD/prompt.
    pub async fn format_for_prompt(&self) -> String {
        let data = self.data.lock().await;
        if data.nodes.is_empty() {
            return "No active goals.".into();
        }

        let roots: Vec<&GoalNode> = data.nodes.iter()
            .filter(|n| n.depth == 0 && !matches!(n.status, GoalStatus::Completed | GoalStatus::Failed))
            .collect();

        if roots.is_empty() {
            return "No active goals.".into();
        }

        let mut out = String::new();
        for root in roots {
            let priority_label = if root.priority >= 0.8 { "HIGH" }
                else if root.priority >= 0.5 { "MED" }
                else { "LOW" };
            out.push_str(&format!(
                "🎯 [{}] {} (progress: {:.0}%)\n",
                priority_label,
                root.title,
                root.progress * 100.0
            ));
            Self::format_children(&data, &root.id, &mut out, 1);
        }
        out
    }

    // ─── Internal Helpers ──────────────────────────────────────────────

    fn format_children(data: &GoalTreeData, parent_id: &str, out: &mut String, indent: usize) {
        let children: Vec<&GoalNode> = data.nodes.iter()
            .filter(|n| n.parent_id.as_deref() == Some(parent_id))
            .collect();

        for child in children {
            let prefix = "  ".repeat(indent);
            let icon = match child.status {
                GoalStatus::Completed => "✅",
                GoalStatus::Active => "🔄",
                GoalStatus::Failed => "❌",
                GoalStatus::Blocked => "🚫",
                GoalStatus::Pending => "⬜",
            };
            let progress_str = format!(", {:.0}%", child.progress * 100.0);
            out.push_str(&format!("{}└─ {} {} ({}{})\n", prefix, icon, child.title, 
                match child.status {
                    GoalStatus::Completed => "DONE",
                    GoalStatus::Active => "IN PROGRESS",
                    GoalStatus::Failed => "FAILED",
                    GoalStatus::Blocked => "BLOCKED",
                    GoalStatus::Pending => "PENDING",
                },
                progress_str
            ));
            Self::format_children(data, &child.id, out, indent + 1);
        }
    }

    fn recalc_progress(data: &mut GoalTreeData, parent_id: &str) {
        let children: Vec<f64> = data.nodes.iter()
            .filter(|n| n.parent_id.as_deref() == Some(parent_id))
            .map(|n| n.progress)
            .collect();

        if children.is_empty() {
            return;
        }

        let avg = children.iter().sum::<f64>() / children.len() as f64;
        let all_complete = children.iter().all(|p| *p >= 1.0);

        if let Some(parent) = data.nodes.iter_mut().find(|n| n.id == parent_id) {
            parent.progress = avg;
            parent.updated_at = now_ts();
            if all_complete {
                parent.status = GoalStatus::Completed;
                parent.progress = 1.0;
            }
            let grandparent = parent.parent_id.clone();
            if let Some(gp_id) = grandparent {
                // Can't recursively call with &mut data, so we'll just do one level.
                // The next update will cascade further if needed.
                let gp_children: Vec<f64> = data.nodes.iter()
                    .filter(|n| n.parent_id.as_deref() == Some(&gp_id))
                    .map(|n| n.progress)
                    .collect();
                if !gp_children.is_empty() {
                    let gp_avg = gp_children.iter().sum::<f64>() / gp_children.len() as f64;
                    if let Some(gp) = data.nodes.iter_mut().find(|n| n.id == gp_id) {
                        gp.progress = gp_avg;
                        gp.updated_at = now_ts();
                        if gp_children.iter().all(|p| *p >= 1.0) {
                            gp.status = GoalStatus::Completed;
                            gp.progress = 1.0;
                        }
                    }
                }
            }
        }
    }

    fn collect_subtree_ids(data: &GoalTreeData, root_id: &str) -> Vec<String> {
        let mut ids = vec![root_id.to_string()];
        let children: Vec<String> = data.nodes.iter()
            .filter(|n| n.parent_id.as_deref() == Some(root_id))
            .map(|n| n.id.clone())
            .collect();
        for child_id in children {
            ids.extend(Self::collect_subtree_ids(data, &child_id));
        }
        ids
    }
}

// ─── Goal Store (multi-scope cache) ────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages goal trees across all scopes. Lazily loads per-scope trees.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_tree_crud() {
        let _ = std::fs::remove_file("/tmp/hive_test_goals/memory/core/goals/test_crud.json");
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_crud");
        
        // Create root goal
        let id = tree.add_root_goal(
            "Test Goal".into(),
            "A test goal for unit testing".into(),
            0.8,
            GoalSource::User,
            vec!["test".into()],
        ).await;
        
        assert!(!id.is_empty());
        
        // Get goal
        let goal = tree.get_goal(&id).await.unwrap();
        assert_eq!(goal.title, "Test Goal");
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.depth, 0);
        
        // Active roots
        let roots = tree.get_active_roots().await;
        assert_eq!(roots.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_tree_subgoals() {
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_subgoals");
        
        let root_id = tree.add_root_goal(
            "Root".into(), "Root goal".into(), 0.9, GoalSource::User, vec![],
        ).await;
        
        let sub_id = tree.add_subgoal(&root_id, "Sub 1".into(), "First subgoal".into(), 0.7, vec![]).await;
        assert!(sub_id.is_some());
        
        let sub = tree.get_goal(sub_id.as_ref().unwrap()).await.unwrap();
        assert_eq!(sub.depth, 1);
        assert_eq!(sub.parent_id.as_deref(), Some(root_id.as_str()));
        
        // Parent should have child registered
        let root = tree.get_goal(&root_id).await.unwrap();
        assert!(root.children.contains(sub_id.as_ref().unwrap()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_progress_bubbling() {
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_progress");
        
        let root_id = tree.add_root_goal(
            "Root".into(), "Root".into(), 0.9, GoalSource::User, vec![],
        ).await;
        
        let sub1_id = tree.add_subgoal(&root_id, "Sub 1".into(), "".into(), 0.5, vec![]).await.unwrap();
        let sub2_id = tree.add_subgoal(&root_id, "Sub 2".into(), "".into(), 0.5, vec![]).await.unwrap();
        
        // Complete sub1
        tree.update_status(&sub1_id, GoalStatus::Completed).await;
        
        // Root should be at 50% (1 of 2 children complete)
        let root = tree.get_goal(&root_id).await.unwrap();
        assert!((root.progress - 0.5).abs() < 0.01);
        
        // Complete sub2
        tree.update_status(&sub2_id, GoalStatus::Completed).await;
        
        // Root should auto-complete
        let root = tree.get_goal(&root_id).await.unwrap();
        assert_eq!(root.status, GoalStatus::Completed);
        assert!((root.progress - 1.0).abs() < 0.01);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_prune() {
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_prune");
        
        let root_id = tree.add_root_goal(
            "Done Goal".into(), "".into(), 0.5, GoalSource::User, vec![],
        ).await;
        let _sub_id = tree.add_subgoal(&root_id, "Sub".into(), "".into(), 0.5, vec![]).await;
        
        // Mark root completed
        tree.update_status(&root_id, GoalStatus::Completed).await;
        
        let pruned = tree.prune_completed().await;
        assert_eq!(pruned, 2); // root + sub
        
        let (total, _) = tree.stats().await;
        assert_eq!(total, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_format() {
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_format");
        
        let root_id = tree.add_root_goal(
            "Learn Rust".into(), "".into(), 0.9, GoalSource::User, vec![],
        ).await;
        let _sub_id = tree.add_subgoal(&root_id, "Read the book".into(), "".into(), 0.5, vec![]).await;
        
        let prompt = tree.format_for_prompt().await;
        assert!(prompt.contains("Learn Rust"));
        assert!(prompt.contains("Read the book"));
        assert!(prompt.contains("🎯"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_evidence() {
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_evidence");
        
        let root_id = tree.add_root_goal(
            "Research".into(), "".into(), 0.5, GoalSource::Autonomy, vec![],
        ).await;
        
        tree.add_evidence(&root_id, "Found 3 papers on the topic".into(), 0.3).await;
        
        let goal = tree.get_goal(&root_id).await.unwrap();
        assert!((goal.progress - 0.3).abs() < 0.01);
        assert_eq!(goal.evidence.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_goal_actionable() {
        let _ = std::fs::remove_file("/tmp/hive_test_goals/memory/core/goals/test_actionable.json");
        let tree = GoalTree::new("/tmp/hive_test_goals", "test_actionable");
        
        let root_id = tree.add_root_goal(
            "Big Goal".into(), "".into(), 0.9, GoalSource::User, vec![],
        ).await;
        let sub_id = tree.add_subgoal(&root_id, "Leaf Task".into(), "".into(), 0.5, vec![]).await.unwrap();
        
        // Root has children, so it's not actionable. Leaf is actionable.
        let actionable = tree.get_actionable().await;
        assert_eq!(actionable.len(), 1);
        assert_eq!(actionable[0].id, sub_id);
    }
}
