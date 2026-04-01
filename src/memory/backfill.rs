//! Background backfill — indexes existing memory data into the vector index.
//!
//! These functions are available for Rust-side backfill but the primary
//! backfill is run via backfill_vectors.py. Kept for future integration.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::providers::embed::EmbedClient;
use crate::memory::vector_index::{VectorIndex, VectorEntry, SourceType};

/// Run one backfill cycle. Processes up to `batch_size` un-indexed entries.
/// Returns the number of newly indexed entries.
pub async fn backfill_embeddings(
    embed_client: &EmbedClient,
    vector_index: &Arc<VectorIndex>,
    memory_dir: &Path,
    batch_size: usize,
) -> usize {
    let vectors_dir = memory_dir.join("vectors");
    let _ = tokio::fs::create_dir_all(&vectors_dir).await;

    let mut total_indexed = 0;

    // ── Timeline backfill ──────────────────────────────────────────
    total_indexed += backfill_timelines(embed_client, vector_index, memory_dir, batch_size).await;

    // ── Synaptic backfill ──────────────────────────────────────────
    total_indexed += backfill_synaptic(embed_client, vector_index, memory_dir, batch_size).await;

    // ── Lessons backfill ───────────────────────────────────────────
    total_indexed += backfill_lessons(embed_client, vector_index, memory_dir, batch_size).await;

    if total_indexed > 0 {
        vector_index.save().await;
        tracing::info!("[BACKFILL] ✅ Indexed {} entries this cycle.", total_indexed);
    }

    total_indexed
}

/// Scan all timeline.jsonl files and embed any un-indexed entries.
async fn backfill_timelines(
    embed_client: &EmbedClient,
    vector_index: &Arc<VectorIndex>,
    memory_dir: &Path,
    batch_size: usize,
) -> usize {
    let mut indexed = 0;

    // Find all timeline.jsonl files recursively
    let timeline_files = find_files_recursive(memory_dir, "timeline.jsonl").await;

    for tl_path in &timeline_files {
        if indexed >= batch_size {
            break;
        }

        let content = match tokio::fs::read_to_string(tl_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Derive a scope key from the path for unique IDs
        let scope_key = tl_path.parent()
            .map(|p| p.strip_prefix(memory_dir).unwrap_or(p))
            .map(|p| p.to_string_lossy().replace('/', ":"))
            .unwrap_or_else(|| "unknown".to_string());

        for (line_idx, line) in content.lines().enumerate() {
            if indexed >= batch_size {
                break;
            }

            let entry_id = format!("timeline:{}:{}", scope_key, line_idx);

            // Skip if already indexed
            if vector_index.contains(&entry_id).await {
                continue;
            }

            // Parse the JSONL entry to extract text
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                let author = event.get("author_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let content = event.get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let timestamp = event.get("timestamp")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if content.is_empty() || content.starts_with("***") {
                    continue; // Skip system/empty entries
                }

                let text = format!("{}: {}", author, content);
                let preview = text.chars().take(200).collect::<String>();

                match embed_client.embed(&text).await {
                    Ok(vec) => {
                        vector_index.insert(VectorEntry {
                            id: entry_id,
                            source: SourceType::Timeline,
                            text_preview: preview,
                            vector: vec,
                            timestamp,
                        }).await;
                        indexed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("[BACKFILL] Embed failed for timeline entry: {}", e);
                        return indexed; // Stop on error (model may be busy)
                    }
                }
            }
        }
    }

    indexed
}

/// Backfill synaptic graph nodes.
async fn backfill_synaptic(
    embed_client: &EmbedClient,
    vector_index: &Arc<VectorIndex>,
    memory_dir: &Path,
    batch_size: usize,
) -> usize {
    let mut indexed = 0;
    let nodes_path = memory_dir.join("synaptic/nodes.jsonl");

    if !nodes_path.exists() {
        return 0;
    }

    let content = match tokio::fs::read_to_string(&nodes_path).await {
        Ok(c) => c,
        Err(_) => return 0,
    };

    for line in content.lines() {
        if indexed >= batch_size {
            break;
        }

        if let Ok(node) = serde_json::from_str::<serde_json::Value>(line) {
            let concept = node.get("concept")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let data = node.get("data")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("; "))
                .unwrap_or_default();

            if concept.is_empty() {
                continue;
            }

            let entry_id = format!("synaptic:{}", concept.to_lowercase());
            if vector_index.contains(&entry_id).await {
                continue;
            }

            let text = format!("{}: {}", concept, data);
            let preview = text.chars().take(200).collect::<String>();

            match embed_client.embed(&text).await {
                Ok(vec) => {
                    let timestamp = node.get("updated_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    vector_index.insert(VectorEntry {
                        id: entry_id,
                        source: SourceType::Synaptic,
                        text_preview: preview,
                        vector: vec,
                        timestamp,
                    }).await;
                    indexed += 1;
                }
                Err(e) => {
                    tracing::warn!("[BACKFILL] Embed failed for synaptic node: {}", e);
                    return indexed;
                }
            }
        }
    }

    indexed
}

/// Backfill lesson entries.
async fn backfill_lessons(
    embed_client: &EmbedClient,
    vector_index: &Arc<VectorIndex>,
    memory_dir: &Path,
    batch_size: usize,
) -> usize {
    let mut indexed = 0;
    let lesson_files = find_files_recursive(memory_dir, "lessons.jsonl").await;

    for lesson_path in &lesson_files {
        if indexed >= batch_size {
            break;
        }

        let content = match tokio::fs::read_to_string(lesson_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let scope_key = lesson_path.parent()
            .map(|p| p.strip_prefix(memory_dir).unwrap_or(p))
            .map(|p| p.to_string_lossy().replace('/', ":"))
            .unwrap_or_else(|| "unknown".to_string());

        for (line_idx, line) in content.lines().enumerate() {
            if indexed >= batch_size {
                break;
            }

            if let Ok(lesson) = serde_json::from_str::<serde_json::Value>(line) {
                let id_val = lesson.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let entry_id = if id_val.is_empty() {
                    format!("lesson:{}:{}", scope_key, line_idx)
                } else {
                    format!("lesson:{}", id_val)
                };

                if vector_index.contains(&entry_id).await {
                    continue;
                }

                let text = lesson.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if text.is_empty() {
                    continue;
                }

                let preview = text.chars().take(200).collect::<String>();

                match embed_client.embed(text).await {
                    Ok(vec) => {
                        let timestamp = lesson.get("learned_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        vector_index.insert(VectorEntry {
                            id: entry_id,
                            source: SourceType::Lesson,
                            text_preview: preview,
                            vector: vec,
                            timestamp,
                        }).await;
                        indexed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("[BACKFILL] Embed failed for lesson: {}", e);
                        return indexed;
                    }
                }
            }
        }
    }

    indexed
}

/// Recursively find files with a specific name under a directory.
async fn find_files_recursive(dir: &Path, filename: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&current).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.file_name().map(|f| f == filename).unwrap_or(false) {
                    results.push(path);
                }
            }
        }
    }

    results
}
