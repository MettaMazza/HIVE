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
#[path = "turing_grid_tests.rs"]
mod tests;
