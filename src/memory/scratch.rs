use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use crate::models::scope::Scope;
#[derive(Debug, Clone)]
pub struct Scratchpad {
    base_dir: PathBuf,
}

impl Default for Scratchpad {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Scratchpad {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        #[cfg(test)]
        let default_dir = std::env::temp_dir().join(format!("hive_mem_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        #[cfg(not(test))]
        let default_dir = PathBuf::from("memory");

        Self {
            base_dir: base_dir.unwrap_or(default_dir),
        }
    }

    /// Gets the specific scratchpad file path for a scope
    fn get_scratchpad_path(&self, scope: &Scope) -> PathBuf {
        let mut path = self.base_dir.clone();
        match scope {
            Scope::Public { channel_id, .. } => path.push(format!("public_{}", channel_id)),
            Scope::Private { user_id } => path.push(format!("private_{}", user_id)),
        }
        path.push("scratch");
        path.push("workspace.md");
        path
    }

    /// Reads the entire contents of the scratchpad. Returns an empty string if it doesn't exist.
    pub async fn read(&self, scope: &Scope) -> String {
        let path = self.get_scratchpad_path(scope);
        fs::read_to_string(&path).await.unwrap_or_default()
    }

    /// Overwrites the scratchpad with new content.
    pub async fn write(&self, scope: &Scope, content: &str) -> std::io::Result<()> {
        let path = self.get_scratchpad_path(scope);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, content).await
    }

    /// Appends text to the end of the scratchpad.
    pub async fn append(&self, scope: &Scope, content: &str) -> std::io::Result<()> {
        let path = self.get_scratchpad_path(scope);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        
        file.write_all(format!("{}\n", content).as_bytes()).await
    }
    
    /// Clears the scratchpad completely.
    pub async fn clear(&self, scope: &Scope) -> std::io::Result<()> {
        let path = self.get_scratchpad_path(scope);
        if path.exists() {
            fs::remove_file(&path).await
        } else {
            Ok(())
        }
    }

    /// Reads the entire contents of the scratchpad. Returns an empty string if it doesn't exist.
    pub async fn search(&self, _query: &str) -> Vec<String> {
        // Stub for retrieving Neo4j conceptual nodes
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scratch_operations() {
        let pub_scope = Scope::Private { user_id: format!("test_scratch_ops_pub_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()) };
        let scratch = Scratchpad::new(None);
        let _ = scratch.clear(&pub_scope).await;
        
        let scratch_default = Scratchpad::default();
        let _ = scratch_default.clear(&pub_scope).await;
        // Call it a second time to hit the `Ok(())` branch when the file doesn't exist anymore
        let _ = scratch_default.clear(&pub_scope).await;
        assert_eq!(scratch_default.read(&pub_scope).await, "");

        let scratch = Scratchpad::new(None);
        
        // Write
        scratch.write(&pub_scope, "First note").await.unwrap();
        assert_eq!(scratch.read(&pub_scope).await, "First note");
        
        // Append
        scratch.append(&pub_scope, "Second note").await.unwrap();
        assert_eq!(scratch.read(&pub_scope).await, "First noteSecond note\n");
        
        // Clear
        scratch.clear(&pub_scope).await.unwrap();
        assert_eq!(scratch.read(&pub_scope).await, "");
    }

    #[tokio::test]
    async fn test_scratchpad_default_and_search() {
        let scratch1 = Scratchpad::default();
        let scratch2 = Scratchpad::new(None);
        let res = scratch1.search("test").await;
        assert!(res.is_empty());
        let _ = scratch2;
    }

    #[tokio::test]
    async fn test_scratch_private_scope() {
        let scratch = Scratchpad::new(None);
        let uid_a = format!("userA_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let uid_b = format!("userB_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let priv_scope = Scope::Private { user_id: uid_a };
        let other_priv = Scope::Private { user_id: uid_b };

        scratch.write(&priv_scope, "Secret A").await.unwrap();
        assert_eq!(scratch.read(&priv_scope).await, "Secret A");
        assert_eq!(scratch.read(&other_priv).await, "");
    }
}
