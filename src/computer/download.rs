use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use futures_util::stream::StreamExt;

/// Performs an HTTP HEAD request to determine the file size before downloading.
/// Returns Some(bytes) if Content-Length is provided, else None.
pub async fn get_file_size(url: &str) -> Option<u64> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
        
    let response = client.head(url).send().await.ok()?;
    if response.status().is_success() {
        response.content_length()
    } else {
        None
    }
}

/// Downloads a file from a URL into the target directory, streaming to disk.
/// Returns the absolute path to the saved file.
pub async fn download_file(url: &str, target_dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(target_dir).await?;

    let client = reqwest::Client::builder()
        // No timeout for giant downloads
        .build()
        .map_err(|e| std::io::Error::other(format!("HTTP client build failed: {}", e)))?;

    let response = client.get(url)
        .send()
        .await
        .map_err(|e| std::io::Error::other(format!("Download failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(std::io::Error::other(format!(
            "HTTP {} from {}", response.status(), url
        )));
    }

    // Check content-length if available (50GB limit)
    if let Some(cl) = response.content_length() {
        if cl > 50_000 * 1024 * 1024 { // 50GB
            return Err(std::io::Error::other(format!(
                "File too large: {} bytes (max 50GB)", cl
            )));
        }
    }

    // Extract filename from Content-Disposition or URL path
    let filename = extract_filename(&response, url);

    let file_path = target_dir.join(&filename);
    
    // Stream the response body directly to disk in chunks
    let mut file = fs::File::create(&file_path).await?;
    let mut total_bytes: u64 = 0;
    const MAX_SIZE: u64 = 50_000 * 1024 * 1024; // 50GB

    // Use hyper::body::Stream to stream chunks
    let mut stream = response.bytes_stream();
    
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| std::io::Error::other(format!("Download stream error: {}", e)))?;
        
        // Check size limit before writing each chunk
        if total_bytes + chunk.len() as u64 > MAX_SIZE {
            // Drop file to close it, then remove
            drop(file);
            tokio::fs::remove_file(&file_path).await.ok();
            return Err(std::io::Error::other(format!(
                "File too large: exceeded 50GB limit during download"
            )));
        }
        
        file.write_all(&chunk).await?;
        total_bytes += chunk.len() as u64;
    }

    file.flush().await?;
    tracing::info!("[DOWNLOAD] ✅ Saved {} ({} bytes) to {:?}", filename, total_bytes, file_path);

    Ok(file_path.canonicalize().unwrap_or(file_path))
}

/// Extracts a filename from the response headers or falls back to the URL path.
fn extract_filename(response: &reqwest::Response, url: &str) -> String {
    // Try Content-Disposition header first
    if let Some(cd) = response.headers().get("content-disposition") {
        if let Ok(cd_str) = cd.to_str() {
            if let Some(start) = cd_str.find("filename=") {
                let name = &cd_str[start + 9..];
                let name = name.trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return sanitize_filename(name);
                }
            }
        }
    }

    // Fall back to URL path
    if let Ok(parsed) = url.parse::<reqwest::Url>() {
        if let Some(segments) = parsed.path_segments() {
            if let Some(last) = segments.last() {
                if !last.is_empty() {
                    return sanitize_filename(last);
                }
            }
        }
    }

    // Final fallback
    format!("download_{}", chrono::Utc::now().timestamp())
}

/// Strips dangerous characters from filenames.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("file.pdf"), "file.pdf");
        assert_eq!(sanitize_filename("bad/path/../file.exe"), "bad_path_.._file.exe");
        assert_eq!(sanitize_filename("normal-file_v2.tar.gz"), "normal-file_v2.tar.gz");
    }

    #[test]
    fn test_extract_filename_from_url() {
        let url = "https://example.com/files/report.pdf";
        // Build a mock response - we can't easily mock reqwest::Response,
        // so just test the URL fallback path via sanitize
        let segments: Vec<&str> = url.split('/').collect();
        let last = segments.last().unwrap();
        assert_eq!(sanitize_filename(last), "report.pdf");
    }
}
