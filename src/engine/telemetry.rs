use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMetrics {
    pub timestamp: String,
    pub model: String,
    pub prompt_bytes: usize,
    pub history_len: usize,
    pub ttft_ms: u64,
    pub total_ms: u64,
    pub prompt_tokens: u64,
    pub eval_tokens: u64,
}

pub async fn log_latency_to_path(metrics: LatencyMetrics, path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_string(&metrics) {
        let log_line = format!("{}\n", json);
        use tokio::io::AsyncWriteExt;
        if let Ok(mut f) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await 
        {
            let _ = f.write_all(log_line.as_bytes()).await;
        }
    }
}

pub async fn log_latency(metrics: LatencyMetrics) {
    tracing::debug!("[ENGINE:Telemetry] 📊 Latency metric: model={} ttft={}ms total={}ms prompt_tokens={} eval_tokens={} history_len={}",
        metrics.model, metrics.ttft_ms, metrics.total_ms, metrics.prompt_tokens, metrics.eval_tokens, metrics.history_len);
    log_latency_to_path(metrics, std::path::Path::new("logs/telemetry.jsonl")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_latency_to_path() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("telemetry.jsonl");

        let metrics = LatencyMetrics {
            timestamp: "2026-03-14T10:00:00Z".to_string(),
            model: "qwen".to_string(),
            prompt_bytes: 100,
            history_len: 1,
            ttft_ms: 10,
            total_ms: 50,
            prompt_tokens: 10,
            eval_tokens: 20,
        };

        log_latency_to_path(metrics, &file_path).await;

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("qwen"));
        assert!(content.contains("2026-03-14T10:00:00Z"));
    }
}
