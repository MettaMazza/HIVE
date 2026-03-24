use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

/// A snapshot of a cell's previous state, used for undo/version history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellSnapshot {
    pub content: String,
    pub format: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub content: String,
    pub format: String,
    pub status: String,
    pub last_updated: String,
    /// Whether a background daemon is looping this cell.
    #[serde(default)]
    pub daemon_active: bool,
    /// Outgoing links to other cells by coordinate.
    #[serde(default)]
    pub links: Vec<(i32, i32, i32)>,
    /// Version history stack (max 3 deep). Most recent first.
    #[serde(default)]
    pub history: Vec<CellSnapshot>,
}

const MAX_HISTORY: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuringGrid {
    pub cells: HashMap<String, Cell>,
    pub cursor: (i32, i32, i32),
    /// Named bookmarks mapping label names to coordinates.
    #[serde(default)]
    pub labels: HashMap<String, (i32, i32, i32)>,
    #[serde(skip)]
    pub persistence_path: PathBuf,
}

impl Default for TuringGrid {
    fn default() -> Self {
        Self::new(PathBuf::from("memory/turing_grid.json"))
    }
}

impl TuringGrid {
    pub fn new(persistence_path: PathBuf) -> Self {
        Self {
            cells: HashMap::new(),
            cursor: (0, 0, 0),
            labels: HashMap::new(),
            persistence_path,
        }
    }

    fn coord_key(x: i32, y: i32, z: i32) -> String {
        format!("{},{},{}", x, y, z)
    }

    pub async fn load(persistence_path: PathBuf) -> std::io::Result<Self> {
        if persistence_path.exists() {
            let data = fs::read_to_string(&persistence_path).await?;
            if let Ok(mut grid) = serde_json::from_str::<TuringGrid>(&data) {
                grid.persistence_path = persistence_path;
                return Ok(grid);
            }
        }
        Ok(Self::new(persistence_path))
    }

    pub async fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.persistence_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_string_pretty(&self)?;
        fs::write(&self.persistence_path, data).await?;
        Ok(())
    }

    pub async fn move_cursor(&mut self, dx: i32, dy: i32, dz: i32) {
        self.cursor.0 += dx;
        self.cursor.1 += dy;
        self.cursor.2 += dz;
        
        // Clamp boundaries to prevent insane numbers
        self.cursor.0 = self.cursor.0.clamp(-2000, 2000);
        self.cursor.1 = self.cursor.1.clamp(-2000, 2000);
        self.cursor.2 = self.cursor.2.clamp(-2000, 2000);
        
        let _ = self.save().await; // Auto-persist on move
    }

    pub fn read_current(&self) -> Option<&Cell> {
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        self.cells.get(&key)
    }

    pub fn get_cursor(&self) -> (i32, i32, i32) {
        self.cursor
    }

    pub async fn write_current(&mut self, format: &str, content: &str) -> std::io::Result<()> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        
        // Push previous content to history before overwriting
        let (old_links, old_history) = if let Some(existing) = self.cells.get(&key) {
            let snapshot = CellSnapshot {
                content: existing.content.clone(),
                format: existing.format.clone(),
                timestamp: existing.last_updated.clone(),
            };
            let mut hist = existing.history.clone();
            hist.insert(0, snapshot);
            hist.truncate(MAX_HISTORY);
            (existing.links.clone(), hist)
        } else {
            (Vec::new(), Vec::new())
        };

        let cell = Cell {
            content: content.to_string(),
            format: format.to_string(),
            status: "Idle".to_string(),
            last_updated: timestamp,
            daemon_active: false,
            links: old_links,
            history: old_history,
        };
        self.cells.insert(key, cell);
        self.save().await
    }
    
    pub async fn update_status(&mut self, status: &str) -> std::io::Result<()> {
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        if let Some(cell) = self.cells.get_mut(&key) {
            cell.status = status.to_string();
            cell.last_updated = chrono::Utc::now().to_rfc3339();
            return self.save().await;
        }
        Ok(())
    }

    pub async fn set_daemon_active(&mut self, active: bool) -> std::io::Result<bool> {
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        if let Some(cell) = self.cells.get_mut(&key) {
            if active && cell.daemon_active {
                return Ok(false);
            }
            cell.daemon_active = active;
            cell.last_updated = chrono::Utc::now().to_rfc3339();
            self.save().await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn scan(&self, radius: i32) -> Vec<((i32, i32, i32), String)> {
        let mut results = Vec::new();
        let (cx, cy, cz) = self.cursor;
        
        for (key, cell) in &self.cells {
            let parts: Vec<&str> = key.split(',').collect();
            if parts.len() == 3
                && let (Ok(x), Ok(y), Ok(z)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>(), parts[2].parse::<i32>())
                    && (x - cx).abs() <= radius && (y - cy).abs() <= radius && (z - cz).abs() <= radius {
                        results.push(((x, y, z), cell.format.clone()));
                    }
        }
        results
    }

    // ──────────────────────────────────────────────
    //  NEW: Index / Manifest
    // ──────────────────────────────────────────────

    /// Generates a virtual index (manifest) of all non-empty cells.
    /// Returns a formatted summary with coordinates, labels, format, link count, and content preview.
    pub fn get_index(&self) -> String {
        if self.cells.is_empty() {
            return "The Turing Grid is empty. No cells have been written.".to_string();
        }

        // Build reverse lookup: coord -> label names
        let mut coord_to_labels: HashMap<String, Vec<String>> = HashMap::new();
        for (name, &(x, y, z)) in &self.labels {
            let key = Self::coord_key(x, y, z);
            coord_to_labels.entry(key).or_default().push(name.clone());
        }

        let mut entries: Vec<String> = Vec::new();
        let mut sorted_keys: Vec<&String> = self.cells.keys().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            let cell = &self.cells[key];
            let preview: String = cell.content.chars().take(80).collect();
            let preview = preview.replace('\n', " ");
            let label_tags = coord_to_labels
                .get(key)
                .map(|names| format!(" 🏷️ {}", names.join(", ")))
                .unwrap_or_default();
            let link_info = if cell.links.is_empty() {
                String::new()
            } else {
                format!(" | {} link(s)", cell.links.len())
            };
            entries.push(format!(
                "• ({}) [{}{}]{} — {}",
                key, cell.format, link_info, label_tags, preview
            ));
        }

        let label_section = if self.labels.is_empty() {
            String::new()
        } else {
            let mut label_lines: Vec<String> = self.labels.iter()
                .map(|(name, (x, y, z))| format!("  🏷️ \"{}\" → ({},{},{})", name, x, y, z))
                .collect();
            label_lines.sort();
            format!("\n\nBookmarks:\n{}", label_lines.join("\n"))
        };

        format!(
            "--- Turing Grid Index ({} cells) ---\nCursor: ({},{},{})\n\n{}{}",
            self.cells.len(),
            self.cursor.0, self.cursor.1, self.cursor.2,
            entries.join("\n"),
            label_section
        )
    }

    // ──────────────────────────────────────────────
    //  NEW: Labels / Bookmarks
    // ──────────────────────────────────────────────

    /// Tags the current cursor position with a named label.
    pub async fn set_label(&mut self, name: &str) -> std::io::Result<()> {
        self.labels.insert(name.to_string(), self.cursor);
        self.save().await
    }

    /// Moves the cursor to a previously labeled position.
    /// Returns Some(coords) on success, None if label not found.
    pub async fn goto_label(&mut self, name: &str) -> Option<(i32, i32, i32)> {
        if let Some(&coords) = self.labels.get(name) {
            self.cursor = coords;
            let _ = self.save().await;
            Some(coords)
        } else {
            None
        }
    }

    // ──────────────────────────────────────────────
    //  NEW: Cell Linking
    // ──────────────────────────────────────────────

    /// Adds a directional link from the current cell to the target coordinates.
    pub async fn add_link(&mut self, target: (i32, i32, i32)) -> std::io::Result<bool> {
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        if let Some(cell) = self.cells.get_mut(&key) {
            if !cell.links.contains(&target) {
                cell.links.push(target);
                self.save().await?;
            }
            Ok(true)
        } else {
            // No cell at current position
            Ok(false)
        }
    }

    // ──────────────────────────────────────────────
    //  NEW: Cell History / Undo
    // ──────────────────────────────────────────────

    /// Returns the version history for the cell at the current cursor position.
    pub fn get_history(&self) -> Option<&Vec<CellSnapshot>> {
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        self.cells.get(&key).map(|c| &c.history)
    }

    /// Restores the most recent history snapshot for the current cell.
    /// Returns true on success, false if no history available.
    pub async fn undo(&mut self) -> std::io::Result<bool> {
        let key = Self::coord_key(self.cursor.0, self.cursor.1, self.cursor.2);
        if let Some(cell) = self.cells.get_mut(&key)
            && let Some(snapshot) = cell.history.first().cloned() {
                cell.content = snapshot.content;
                cell.format = snapshot.format;
                cell.last_updated = chrono::Utc::now().to_rfc3339();
                cell.history.remove(0);
                self.save().await?;
                return Ok(true);
            }
        Ok(false)
    }

    // ──────────────────────────────────────────────
    //  NEW: Read cell at arbitrary coords (for pipeline)
    // ──────────────────────────────────────────────

    /// Read a cell at specific coordinates without moving the cursor.
    pub fn read_at(&self, x: i32, y: i32, z: i32) -> Option<&Cell> {
        let key = Self::coord_key(x, y, z);
        self.cells.get(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_grid_initialization() {
        let grid = TuringGrid::new(PathBuf::from("dummy.json"));
        assert_eq!(grid.cursor, (0, 0, 0));
        assert!(grid.cells.is_empty());
        assert!(grid.labels.is_empty());
    }

    #[tokio::test]
    async fn test_grid_movement() {
        let mut grid = TuringGrid::new(PathBuf::from("dummy.json"));
        grid.move_cursor(5, -2, 10).await;
        assert_eq!(grid.get_cursor(), (5, -2, 10));
        
        // Test bounds clamping
        grid.move_cursor(3000, 0, 0).await;
        assert_eq!(grid.get_cursor(), (2000, -2, 10));
    }

    #[tokio::test]
    async fn test_grid_write_and_read() {
        let dir = env::temp_dir().join("hive_turing_test_rw");
        let path = dir.join("turing_grid.json");
        let mut grid = TuringGrid::new(path.clone());
        grid.move_cursor(1, 1, 1).await;
        
        grid.write_current("python", "print('hello 3D')").await.unwrap();
        
        let cell = grid.read_current().unwrap();
        assert_eq!(cell.format, "python");
        assert_eq!(cell.content, "print('hello 3D')");
        assert_eq!(cell.status, "Idle");
        assert_eq!(cell.daemon_active, false);
        assert!(cell.links.is_empty());
        assert!(cell.history.is_empty()); // First write, no history
        
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_grid_scan() {
        let dir = env::temp_dir().join("hive_turing_test_scan");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));
        
        grid.move_cursor(1, 0, 0).await;
        grid.write_current("text", "data 1").await.unwrap();
        
        grid.move_cursor(-2, 0, 0).await; // Cursor now at (-1, 0, 0)
        grid.write_current("sh", "echo 'bash'").await.unwrap();
        
        grid.move_cursor(1, 0, 0).await; // Back to origin (0, 0, 0)
        
        let scan = grid.scan(1);
        assert_eq!(scan.len(), 2);
        
        let scan_small = grid.scan(0);
        assert_eq!(scan_small.len(), 0);
        
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_grid_persistence() {
        let dir = env::temp_dir().join("hive_turing_test_persist");
        fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("turing_grid.json");
        
        let mut grid = TuringGrid::new(path.clone());
        grid.move_cursor(5, 5, 5).await;
        grid.write_current("text", "persistent data").await.unwrap();
        
        let reloaded_grid = TuringGrid::load(path.clone()).await.unwrap();
        assert_eq!(reloaded_grid.cursor, (5, 5, 5));
        
        let cell = reloaded_grid.read_current().unwrap();
        assert_eq!(cell.content, "persistent data");
        
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
    
    #[tokio::test]
    async fn test_update_status() {
        let dir = env::temp_dir().join("hive_turing_test_status");
        let path = dir.join("turing_grid.json");
        let mut grid = TuringGrid::new(path.clone());
        
        grid.write_current("python", "x = 1").await.unwrap();
        grid.update_status("Running").await.unwrap();
        
        let cell = grid.read_current().unwrap();
        assert_eq!(cell.status, "Running");
        
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    // ─── NEW TESTS ─────────────────────────────────

    #[tokio::test]
    async fn test_grid_index_generation() {
        let dir = env::temp_dir().join("hive_turing_test_index");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));

        // Write 3 cells at different locations
        grid.write_current("text", "origin cell").await.unwrap();
        grid.move_cursor(1, 0, 0).await;
        grid.write_current("python", "print('hello')").await.unwrap();
        grid.move_cursor(0, 1, 0).await;
        grid.write_current("json", r#"{"key": "value"}"#).await.unwrap();

        let index = grid.get_index();
        assert!(index.contains("3 cells"));
        assert!(index.contains("0,0,0"));
        assert!(index.contains("1,0,0"));
        assert!(index.contains("1,1,0"));
        assert!(index.contains("origin cell"));
        assert!(index.contains("python"));

        // Empty grid
        let empty = TuringGrid::new(PathBuf::from("dummy.json"));
        assert!(empty.get_index().contains("empty"));

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_grid_labels() {
        let dir = env::temp_dir().join("hive_turing_test_labels");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));

        // Move to a position and label it
        grid.move_cursor(5, 10, -3).await;
        grid.set_label("research_area").await.unwrap();

        // Move away
        grid.move_cursor(-5, -10, 3).await;
        assert_eq!(grid.get_cursor(), (0, 0, 0));

        // Goto the label
        let result = grid.goto_label("research_area").await;
        assert_eq!(result, Some((5, 10, -3)));
        assert_eq!(grid.get_cursor(), (5, 10, -3));

        // Non-existent label
        let none = grid.goto_label("doesnt_exist").await;
        assert_eq!(none, None);

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_grid_labels_persistence() {
        let dir = env::temp_dir().join("hive_turing_test_labels_persist");
        fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("turing_grid.json");

        let mut grid = TuringGrid::new(path.clone());
        grid.move_cursor(3, 7, 1).await;
        grid.set_label("saved_spot").await.unwrap();

        // Reload
        let reloaded = TuringGrid::load(path.clone()).await.unwrap();
        assert_eq!(reloaded.labels.get("saved_spot"), Some(&(3, 7, 1)));

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_cell_linking() {
        let dir = env::temp_dir().join("hive_turing_test_linking");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));

        // Write two cells
        grid.write_current("text", "cell A").await.unwrap();
        grid.move_cursor(1, 0, 0).await;
        grid.write_current("text", "cell B").await.unwrap();

        // Move back to A, link to B
        grid.move_cursor(-1, 0, 0).await;
        let linked = grid.add_link((1, 0, 0)).await.unwrap();
        assert!(linked);

        let cell_a = grid.read_current().unwrap();
        assert_eq!(cell_a.links, vec![(1, 0, 0)]);

        // Duplicate link should not add
        grid.add_link((1, 0, 0)).await.unwrap();
        let cell_a2 = grid.read_current().unwrap();
        assert_eq!(cell_a2.links.len(), 1);

        // Link from empty cell should return false
        grid.move_cursor(99, 99, 99).await;
        let empty_link = grid.add_link((0, 0, 0)).await.unwrap();
        assert!(!empty_link);

        // Verify links appear in index
        let index = grid.get_index();
        assert!(index.contains("1 link(s)"));

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_cell_history_and_undo() {
        let dir = env::temp_dir().join("hive_turing_test_history");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));

        // Write v1
        grid.write_current("text", "version 1").await.unwrap();
        assert!(grid.get_history().unwrap().is_empty());

        // Write v2 — v1 enters history
        grid.write_current("text", "version 2").await.unwrap();
        let hist = grid.get_history().unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].content, "version 1");

        // Write v3 — v2 enters history
        grid.write_current("python", "version 3").await.unwrap();
        let hist = grid.get_history().unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].content, "version 2");
        assert_eq!(hist[1].content, "version 1");

        // Write v4 — v3 enters, history now at max 3
        grid.write_current("text", "version 4").await.unwrap();
        let hist = grid.get_history().unwrap();
        assert_eq!(hist.len(), 3);

        // Write v5 — oldest drops off (max 3)
        grid.write_current("text", "version 5").await.unwrap();
        let hist = grid.get_history().unwrap();
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[0].content, "version 4");

        // Undo — should restore v4
        let undone = grid.undo().await.unwrap();
        assert!(undone);
        let cell = grid.read_current().unwrap();
        assert_eq!(cell.content, "version 4");
        assert_eq!(cell.history.len(), 2);

        // Undo on empty cell
        grid.move_cursor(99, 99, 99).await;
        let empty_undo = grid.undo().await.unwrap();
        assert!(!empty_undo);

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_labels_in_index() {
        let dir = env::temp_dir().join("hive_turing_test_label_index");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));

        grid.write_current("text", "labeled cell").await.unwrap();
        grid.set_label("home").await.unwrap();

        let index = grid.get_index();
        assert!(index.contains("🏷️ \"home\""));
        assert!(index.contains("Bookmarks"));

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_read_at() {
        let dir = env::temp_dir().join("hive_turing_test_read_at");
        let mut grid = TuringGrid::new(dir.join("turing_grid.json"));

        grid.write_current("text", "at origin").await.unwrap();
        grid.move_cursor(5, 5, 5).await;

        // Read origin without moving cursor
        let cell = grid.read_at(0, 0, 0).unwrap();
        assert_eq!(cell.content, "at origin");
        assert_eq!(grid.get_cursor(), (5, 5, 5)); // Cursor unchanged

        // Non-existent cell
        assert!(grid.read_at(99, 99, 99).is_none());

        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
