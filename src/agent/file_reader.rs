use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;

fn extract_tag(desc: &str, tag: &str) -> Option<String> {
    if let Some(start_idx) = desc.find(tag) {
        let after_tag = &desc[start_idx + tag.len()..];
        if after_tag.starts_with('[')
            && let Some(end_idx) = after_tag.find(']') {
                return Some(after_tag[1..end_idx].trim().to_string());
            }
    }
    None
}

pub async fn execute_file_reader(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧠 Native Codebase Reader Tool scanning: {}\n", description)).await;
    }
    tracing::debug!("[AGENT:file_reader] ▶ task_id={} desc_len={}", task_id, description.len());

    let target_path = extract_tag(&description, "name:")
        .unwrap_or_else(|| {
            description
                .split_whitespace()
                .find(|s| s.contains("src/") || s.contains('/') || s.contains('.'))
                .map(|s| s.trim_matches(|c| c == '(' || c == ')' || c == '\'' || c == '"' || c == '`' || c == ','))
                .unwrap_or_else(|| description.split_whitespace().last().unwrap_or(""))
                .to_string()
        });

    let start_line: usize = extract_tag(&description, "start_line:").and_then(|s| s.parse().ok()).unwrap_or(1);
    let limit: usize = extract_tag(&description, "limit:").and_then(|s| s.parse().ok()).unwrap_or(500);
    let query: Option<String> = extract_tag(&description, "query:");

    let output = if target_path.contains("..") || target_path.starts_with('/') {
        "Access Denied: Path traverses outside isolated project root.".to_string()
    } else {
        // Resolve relative to project root — try env var first, then compile-time path, then CWD
        let project_root = std::env::var("HIVE_PROJECT_DIR")
            .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
        let absolute_path = std::path::Path::new(&project_root).join(&target_path);
        let content_result = match tokio::fs::read_to_string(&absolute_path).await {
            Ok(c) => Ok(c),
            Err(_) => tokio::fs::read_to_string(&target_path).await,
        };
        
        let (content, resolved_path) = match content_result {
            Ok(c) => (c, target_path.clone()),
            Err(_) => {
                let filename = std::path::Path::new(&target_path)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&target_path);

                let src_dir = std::path::Path::new(&project_root).join("src");
                let src_str = src_dir.to_string_lossy().to_string();
                let find_result = std::process::Command::new("find")
                    .args(&[&src_str, "-name", filename, "-type", "f"])
                    .output();

                match find_result {
                    Ok(res) => {
                        let found = String::from_utf8_lossy(&res.stdout);
                        let found_path = found.trim().lines().next().unwrap_or("");
                        if !found_path.is_empty() {
                            if let Ok(c) = tokio::fs::read_to_string(found_path).await {
                                (c, found_path.to_string())
                            } else {
                                return ToolResult { task_id, output: format!("Failed to read file: {} (found at {} but read failed)", target_path, found_path), tokens_used: 0, status: ToolStatus::Failed("Read failed".into()) };
                            }
                        } else {
                            return ToolResult { task_id, output: format!("Failed to read file: {} (not found, also searched src/ for '{}')", target_path, filename), tokens_used: 0, status: ToolStatus::Failed("Not found".into()) };
                        }
                    }
                    Err(_) => return ToolResult { task_id, output: format!("Failed to read file: {}", target_path), tokens_used: 0, status: ToolStatus::Failed("Not found".into()) },
                }
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let regex_pattern = extract_tag(&description, "regex:");
        let has_query = query.is_some() || regex_pattern.is_some();
        let compiled_re: Option<regex::Regex> = regex_pattern.as_ref().and_then(|pat| regex::Regex::new(pat).ok());

        if has_query {
            let mut matches = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                let mut matched = false;
                if let Some(ref q) = query {
                    if line.contains(q) { matched = true; }
                }
                if let Some(ref re) = compiled_re {
                    if re.is_match(line) { matched = true; }
                }

                if matched {
                    let start = i.saturating_sub(3);
                    let end = (i + 4).min(total_lines);
                    let chunk = lines[start..end].join("\n");
                    matches.push(format!("--- Match at line {} ---\n{}", i + 1, chunk));
                }
            }
            if matches.is_empty() {
                let empty_q = query.clone().unwrap_or_else(|| regex_pattern.clone().unwrap_or_default());
                format!("File: {}\nNo matches found for query/regex: '[{}]'", resolved_path, empty_q)
            } else {
                let display_matches = if matches.len() > 15 {
                    let mut m = matches[..15].to_vec();
                    m.push(format!("... and {} more matches. Be more specific.", matches.len() - 15));
                    m
                } else {
                    matches
                };
                let query_str = query.unwrap_or_else(|| regex_pattern.clone().unwrap_or_default());
                format!("File: {}\nFound matches for query/regex: '[{}]'\n(NOTE: Copy-paste exact blocks for patching)\n\n{}", resolved_path, query_str, display_matches.join("\n\n"))
            }
        } else {
            let start_idx = start_line.saturating_sub(1).min(total_lines);
            let end_idx = (start_idx + limit).min(total_lines);
            
            let chunked_content = lines[start_idx..end_idx].join("\n");
            let pct = if total_lines > 0 { ((end_idx as f64 / total_lines as f64) * 100.0) as u32 } else { 100 };
            let remaining = total_lines.saturating_sub(end_idx);

            let mut header = format!("File: {}\nLines: {}-{}/{} ({}% complete", resolved_path, start_idx + 1, end_idx, total_lines, pct);
            
            if remaining > 0 {
                header.push_str(&format!(", {} lines remaining)\n[BOOKMARK: Continue with codebase_read(name:[{}] start_line:[{}] limit:[{}])]\n[READING INCOMPLETE — you MUST continue reading before responding]", remaining, target_path, end_idx + 1, limit));
            } else {
                header.push_str(")\n[DOCUMENT COMPLETE]");
            }

            format!("{}\n\n{}", header, chunked_content)
        }
    };

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_file_reader_standard() {
        let res = execute_file_reader("1".into(), "src/main.rs".into(), None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("fn main"));
    }

    #[tokio::test]
    async fn test_execute_file_reader_traversal() {
        let res = execute_file_reader("2".into(), "Read ../../../etc/passwd".into(), None).await;
        assert!(res.output.contains("Access Denied"));
    }

    #[tokio::test]
    async fn test_execute_file_reader_fallback_success() {
        let res = execute_file_reader("3".into(), "Read main.rs to find stuff".into(), None).await;
        assert!(res.output.contains("main.rs"));
        assert!(res.output.contains("fn main"));
    }

    #[tokio::test]
    async fn test_execute_file_reader_fallback_fail() {
        let res = execute_file_reader("4".into(), "Read doesnt_exist_at_all.rs".into(), None).await;
        assert!(res.output.contains("Failed to read file"));
    }
}
