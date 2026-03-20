use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;

pub async fn execute_download(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:")
        .unwrap_or("download".to_string())
        .trim()
        .to_string();

    if action == "status" {
        let filename = extract_tag(&description, "file:")
            .unwrap_or_default()
            .trim()
            .to_string();
            
        if filename.is_empty() {
            return ToolResult {
                task_id,
                output: "Error: Missing file:[filename] for status check.".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("Missing filename".into()),
            };
        }
        
        let file_path = std::path::Path::new("memory/core/downloads").join(&filename);
        if !file_path.exists() {
            return ToolResult {
                task_id,
                output: format!("File {} not found or hasn't started downloading yet.", filename),
                tokens_used: 0,
                status: ToolStatus::Success,
            };
        }
        
        let meta = tokio::fs::metadata(&file_path).await;
        match meta {
            Ok(m) => {
                let size_mb = m.len() as f64 / 1_048_576.0;
                return ToolResult {
                    task_id,
                    output: format!("Download progress for {}: {:.2} MB downloaded so far.", filename, size_mb),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
            Err(e) => {
                return ToolResult {
                    task_id,
                    output: format!("Error checking file: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Metadata error".into()),
                };
            }
        }
    }

    let url = extract_tag(&description, "url:")
        .unwrap_or_default()
        .trim()
        .to_string();

    if url.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing url:[...] in description.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing URL".into()),
        };
    }

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("⬇️ Checking size for {}...\n", url)).await;
    }

    // 25MB threshold for Async download
    let threshold_bytes: u64 = 25 * 1024 * 1024;
    let file_size_opt = crate::computer::download::get_file_size(&url).await;
    
    let is_large = match file_size_opt {
        Some(size) => size > threshold_bytes,
        None => false, // If we can't get size, assume small and let reqwest handle it
    };

    let download_dir = std::path::Path::new("memory/core/downloads").to_path_buf();
    
    if is_large {
        let size_mb = file_size_opt.unwrap_or(0) as f64 / 1_048_576.0;
        if let Some(ref tx) = telemetry_tx {
            let _ = tx.send(format!("📦 Large file detected ({:.2} MB). Spawning background download...\n", size_mb)).await;
        }
        
        // Spawn background task
        let url_clone = url.clone();
        tokio::spawn(async move {
            let _ = crate::computer::download::download_file(&url_clone, &download_dir).await;
        });
        
        return ToolResult {
            task_id,
            output: format!(
                "Background download started for {} ({:.2} MB).\n\n\
                This is a large file, so it will download asynchronously. \
                You can use action:[status] file:[filename] later to check the progress.",
                url, size_mb
            ),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("⬇️ Downloading file...\n".to_string()).await;
    }

    match crate::computer::download::download_file(&url, &download_dir).await {
        Ok(path) => {
            let path_str = path.to_string_lossy().to_string();
            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send("✅ Download complete.\n".to_string()).await;
            }

            // Build the public file server URL (uses tunnel if available)
            let base_url = crate::computer::file_server::get_public_base_url();
            let token = std::env::var("HIVE_FILE_TOKEN").unwrap_or_default();
            let filename = path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let server_url = if token.is_empty() {
                format!("{}/files/{}", base_url, filename)
            } else {
                format!("{}/files/{}?token={}", base_url, filename, token)
            };
            let browser_url = if token.is_empty() {
                format!("{}/files/", base_url)
            } else {
                format!("{}/files/?token={}", base_url, token)
            };

            ToolResult {
                task_id,
                output: format!(
                    "Download complete.\n\
                    Local path: {}\n\
                    📎 Direct download link: {}\n\
                    🌐 File browser (all files): {}\n\n\
                    YOU MUST include ALL of the following in your reply:\n\
                    1. The direct download link so the user can download this file\n\
                    2. The file browser link so the user can see all available files\n\
                    3. This exact attachment tag: [ATTACH_FILE]({})\n\n\
                    If you do not include the links, the user cannot access files remotely.",
                    path_str, server_url, browser_url, path_str
                ),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => {
            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("❌ Download failed: {}\n", e)).await;
            }
            ToolResult {
                task_id,
                output: format!("Download failed: {}", e),
                tokens_used: 0,
                status: ToolStatus::Failed(format!("Download error: {}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_download_missing_url() {
        let res = execute_download("1".into(), "".into(), None).await;
        assert!(matches!(res.status, ToolStatus::Failed(_)));
        assert!(res.output.contains("Missing url"));
    }

    #[tokio::test]
    async fn test_execute_download_invalid_url() {
        let res = execute_download(
            "2".into(),
            "url:[not_a_real_url_at_all]".into(),
            None,
        ).await;
        assert!(matches!(res.status, ToolStatus::Failed(_)));
    }
}
