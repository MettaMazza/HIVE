/// Sleep Training — Biologically-inspired micro-batch LoRA training.
///
/// Simulates sleep-like memory consolidation:
/// - `/sleep` command or 12-hour auto-timer triggers a micro-training cycle
/// - Selects the top 1-2 highest-quality examples from the buffer
/// - Trains with epochs=1, lr=1e-5, batch_size=1 on the PREVIOUS adapter
/// - Each cycle produces an imperceptible weight delta that compounds over time
///
/// The key insight: cumulative stacking. Each sleep trains on top of the last
/// adapter, not the base model. This creates gradual drift, like a human brain
/// slowly consolidating memories during sleep.

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::io::AsyncWriteExt;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::teacher::{Teacher, GoldenExample};
use crate::providers::Provider;
use crate::memory::MemoryStore;
use crate::models::message::Event;
use crate::models::scope::Scope;

/// Sleep configuration — intentionally conservative defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepConfig {
    /// Max examples per micro-training cycle (1-2 is the sweet spot)
    pub micro_batch_size: usize,
    /// Learning rate for micro-training (very low = subtle drift)
    pub micro_lr: f64,
    /// Epochs per micro-cycle (1 = single pass, minimal overfitting)
    pub micro_epochs: usize,
    /// Seconds between automatic sleep cycles (43200 = 12 hours)
    pub auto_sleep_interval_secs: u64,
    /// Max sequence length for micro-training (lower = less VRAM)
    pub micro_max_seq_len: usize,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            micro_batch_size: 2,
            micro_lr: 1e-5,
            micro_epochs: 1,
            auto_sleep_interval_secs: 43200, // 12 hours
            micro_max_seq_len: 8192,
        }
    }
}

impl SleepConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(v) = std::env::var("HIVE_SLEEP_BATCH") {
            if let Ok(n) = v.parse() {
                config.micro_batch_size = n;
            }
        }
        if let Ok(v) = std::env::var("HIVE_SLEEP_LR") {
            if let Ok(lr) = v.parse() {
                config.micro_lr = lr;
            }
        }
        if let Ok(v) = std::env::var("HIVE_SLEEP_INTERVAL") {
            if let Ok(secs) = v.parse() {
                config.auto_sleep_interval_secs = secs;
            }
        }

        config
    }
}

/// Report generated after a sleep cycle completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepReport {
    pub version: String,
    pub parent: String,
    pub golden_used: usize,
    pub pairs_used: usize,
    pub identity_reinforced: bool,
    pub duration_secs: f64,
    pub timestamp: String,
    pub quality_scores: Vec<f64>,
}

impl std::fmt::Display for SleepReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let identity = if self.identity_reinforced { " + identity reflection" } else { "" };
        write!(
            f,
            "☀️ Woke up. Dreamed of {} examples{} → {} (parent: {}, {:.1}s)",
            self.golden_used + self.pairs_used,
            identity,
            self.version,
            self.parent,
            self.duration_secs,
        )
    }
}

/// The sleep cycle orchestrator.
pub struct SleepCycle {
    pub config: SleepConfig,
    pub last_sleep: Arc<Mutex<Option<DateTime<Utc>>>>,
    pub teacher: Arc<Teacher>,
    /// Provider for identity reflection inference (None in tests)
    provider: Option<Arc<dyn Provider>>,
    /// Memory store for reading lessons + synaptic graph (None in tests)
    memory: Option<Arc<MemoryStore>>,
}

impl SleepCycle {
    pub fn new(teacher: Arc<Teacher>, config: Option<SleepConfig>) -> Self {
        Self {
            config: config.unwrap_or_else(SleepConfig::from_env),
            last_sleep: Arc::new(Mutex::new(None)),
            teacher,
            provider: None,
            memory: None,
        }
    }

    /// Create with provider and memory for identity reflection.
    pub fn with_inference(
        teacher: Arc<Teacher>,
        provider: Arc<dyn Provider>,
        memory: Arc<MemoryStore>,
        config: Option<SleepConfig>,
    ) -> Self {
        Self {
            config: config.unwrap_or_else(SleepConfig::from_env),
            last_sleep: Arc::new(Mutex::new(None)),
            teacher,
            provider: Some(provider),
            memory: Some(memory),
        }
    }

    /// Check if it's time for an automatic sleep cycle.
    pub async fn should_auto_sleep(&self) -> bool {
        let last = self.last_sleep.lock().await;
        match *last {
            None => {
                // Never slept — check if we have any training data at all
                let (golden, pairs) = self.teacher.get_counts();
                golden > 0 || pairs > 0
            }
            Some(last_time) => {
                let elapsed = Utc::now() - last_time;
                let threshold = chrono::Duration::seconds(self.config.auto_sleep_interval_secs as i64);
                if elapsed < threshold {
                    return false;
                }
                // Only sleep if there's new data since last sleep
                let (golden, pairs) = self.teacher.get_counts();
                golden > 0 || pairs > 0
            }
        }
    }

    /// Enter sleep: select best examples, train, wake up.
    pub async fn enter_sleep(&self) -> Result<SleepReport, String> {
        let start = std::time::Instant::now();

        // Acquire training lock
        if !self.teacher.try_acquire_training_lock().await {
            return Err("Training lock busy — another sleep cycle or training is in progress".into());
        }

        tracing::info!("💤 [SLEEP] Entering sleep cycle...");

        // Load and rank examples
        let golden = self.load_and_rank_golden()?;
        let (golden_count, pair_count) = self.teacher.get_counts();

        let selected_count = golden.len().min(self.config.micro_batch_size);
        let quality_scores: Vec<f64> = golden.iter()
            .take(selected_count)
            .map(|g| g.quality_score)
            .collect();

        if selected_count == 0 && pair_count == 0 {
            self.teacher.release_training_lock().await;
            return Err("No training data available for sleep cycle".into());
        }

        tracing::info!(
            "💤 [SLEEP] Selected {} golden examples (of {}) + {} pairs for micro-training",
            selected_count, golden_count, pair_count
        );

        // Identity Reflection — Apis reviews her responses and reflects on who she is
        let identity_reinforced = if self.provider.is_some() && selected_count > 0 {
            match self.generate_identity_reflection(&golden, selected_count).await {
                Ok(()) => {
                    tracing::info!("💤 [SLEEP] 🪞 Identity reflection generated and appended to training data");
                    true
                }
                Err(e) => {
                    tracing::warn!("💤 [SLEEP] Identity reflection failed (non-fatal): {}", e);
                    false
                }
            }
        } else {
            false
        };

        // Run micro-training via Python script
        let manifest = self.teacher.load_manifest();
        let parent = manifest.current.clone();

        let python_bin = std::env::var("HIVE_PYTHON_BIN")
            .unwrap_or_else(|_| "python3".to_string());
        let training_backend = std::env::var("HIVE_TRAINING_BACKEND")
            .unwrap_or_else(|_| "auto".to_string());

        // Validate training environment before spawning
        let script_path = std::path::Path::new("training/train_teacher.py");
        if !script_path.exists() {
            self.teacher.release_training_lock().await;
            return Err(format!(
                "Training script not found at '{}'. Ensure the training directory is present.",
                script_path.display()
            ));
        }

        // macOS: Unload the Ollama model to free Metal/IOSurface handles.
        // MLX LoRA training needs Metal buffers, and the system has a hard 1024
        // IOSurface client limit. On Linux/Windows this is unnecessary.
        #[cfg(target_os = "macos")]
        {
            let ollama_host = std::env::var("OLLAMA_HOST")
                .or_else(|_| std::env::var("HIVE_OLLAMA_URL"))
                .unwrap_or_else(|_| "http://localhost:11434".to_string());
            let model_name = std::env::var("HIVE_MODEL")
                .unwrap_or_else(|_| "qwen3.5:35b".to_string());

            tracing::info!("💤 [SLEEP] Unloading Ollama model '{}' to free Metal resources for training...", model_name);
            let unload_payload = serde_json::json!({
                "model": model_name,
                "keep_alive": 0
            });
            let unload_result = reqwest::Client::new()
                .post(format!("{}/api/generate", ollama_host))
                .json(&unload_payload)
                .send()
                .await;
            match &unload_result {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!("💤 [SLEEP] ✅ Ollama model unloaded — Metal resources freed");
                }
                Ok(resp) => {
                    tracing::warn!("💤 [SLEEP] Ollama unload returned {}, proceeding anyway", resp.status());
                }
                Err(e) => {
                    tracing::warn!("💤 [SLEEP] Could not unload Ollama model: {}, proceeding anyway", e);
                }
            }
            // Brief pause to let macOS reclaim IOSurface handles
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        #[cfg(not(target_os = "macos"))]
        {
            tracing::info!("💤 [SLEEP] Non-macOS platform — skipping Ollama model unload");
        }

        tracing::info!("💤 [SLEEP] Launching training: backend={}, python={}", training_backend, python_bin);

        let in_docker = std::path::Path::new("/.dockerenv").exists();

        let (stdout, _stderr) = if in_docker {
            // ── DOCKER: Call the host's training server via HTTP ──
            // Training needs Metal/MLX/GPU — the host machine has it.
            // Same pattern as Ollama (inference) and Flux (images).
            let train_url = std::env::var("HIVE_TRAINING_URL")
                .unwrap_or_else(|_| "http://host.docker.internal:8491".to_string());

            tracing::info!("💤 [SLEEP] Docker detected — calling host training server at {}", train_url);

            let payload = serde_json::json!({
                "micro": true,
                "stack": true,
                "backend": training_backend,
                "examples": selected_count,
                "lr": self.config.micro_lr,
                "epochs": self.config.micro_epochs,
                "max_seq_len": self.config.micro_max_seq_len,
            });

            let client = reqwest::Client::new();
            let resp = match client.post(format!("{}/train", train_url))
                .json(&payload)
                .timeout(std::time::Duration::from_secs(3600))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    self.teacher.release_training_lock().await;
                    return Err(format!(
                        "Failed to reach host training server at {}: {}. \
                         Make sure train_server.py is running on the host (started by launch.sh).",
                        train_url, e
                    ));
                }
            };

            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();

            let out = body["stdout"].as_str().unwrap_or("").to_string();
            let err = body["stderr"].as_str().unwrap_or("").to_string();
            let train_status = body["status"].as_str().unwrap_or("unknown");

            if !status.is_success() || train_status != "success" {
                tracing::error!("💤 [SLEEP] ❌ Host training failed (status={}, server={}):\nstdout: {}\nstderr: {}",
                    status, train_status, out, err);
                self.teacher.release_training_lock().await;
                return Err(format!("Host training failed ({}): {}", train_status, err));
            }

            (out, err)
        } else {
            // ── NATIVE: Run training subprocess directly (MLX/Metal) ──
            let training_future = tokio::process::Command::new(&python_bin)
                .arg("training/train_teacher.py")
                .arg("--micro")
                .arg("--stack")
                .arg("--backend")
                .arg(&training_backend)
                .arg("--examples")
                .arg(selected_count.to_string())
                .arg("--lr")
                .arg(self.config.micro_lr.to_string())
                .arg("--epochs")
                .arg(self.config.micro_epochs.to_string())
                .arg("--max-seq-len")
                .arg(self.config.micro_max_seq_len.to_string())
                .output();

            let output = match training_future.await {
                Ok(o) => o,
                Err(e) => {
                    self.teacher.release_training_lock().await;
                    return Err(format!("Failed to execute training script '{}': {}", python_bin, e));
                }
            };

            let out = String::from_utf8_lossy(&output.stdout).to_string();
            let err = String::from_utf8_lossy(&output.stderr).to_string();

            if !output.status.success() {
                tracing::error!("💤 [SLEEP] ❌ Training failed (exit {}):\nstdout: {}\nstderr: {}",
                    output.status, out, err);
                self.teacher.release_training_lock().await;
                return Err(format!("Micro-training failed (exit {}): {}", output.status, err));
            }

            (out, err)
        };

        let duration = start.elapsed().as_secs_f64();

        // Log training output on success
        if !stdout.is_empty() {
            tracing::info!("💤 [SLEEP] Training output:\n{}", stdout);
        }

        // Read updated manifest
        let updated_manifest = self.teacher.load_manifest();
        let new_version = updated_manifest.current.clone();

        // Reset counters and record sleep time
        self.teacher.reset_counts();
        {
            let mut last = self.last_sleep.lock().await;
            *last = Some(Utc::now());
        }

        self.teacher.release_training_lock().await;

        let report = SleepReport {
            version: new_version,
            parent,
            golden_used: selected_count,
            pairs_used: pair_count.min(self.config.micro_batch_size),
            identity_reinforced,
            duration_secs: duration,
            timestamp: Utc::now().to_rfc3339(),
            quality_scores,
        };

        tracing::info!("☀️ [SLEEP] {}", report);

        Ok(report)
    }

    /// Load golden examples from the buffer and rank by quality score.
    fn load_and_rank_golden(&self) -> Result<Vec<ScoredGolden>, String> {
        let content = std::fs::read_to_string(&self.teacher.golden_path)
            .unwrap_or_default();

        let mut scored: Vec<ScoredGolden> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| {
                let example: GoldenExample = serde_json::from_str(line).ok()?;
                let score = compute_quality_score(&example);
                Some(ScoredGolden { example, quality_score: score })
            })
            .collect();

        // Sort descending by quality score (best first)
        scored.sort_by(|a, b| b.quality_score.partial_cmp(&a.quality_score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored)
    }

    /// Get sleep status for the HUD / user query.
    pub async fn status(&self) -> SleepStatus {
        let last = self.last_sleep.lock().await;
        let (golden, pairs) = self.teacher.get_counts();
        let next_auto = match *last {
            None => Some(Utc::now()), // Would sleep now if data exists
            Some(t) => {
                let next = t + chrono::Duration::seconds(self.config.auto_sleep_interval_secs as i64);
                Some(next)
            }
        };

        SleepStatus {
            last_sleep: *last,
            next_auto_sleep: next_auto,
            golden_buffered: golden,
            pairs_buffered: pairs,
            config: self.config.clone(),
        }
    }

    /// Generate an identity reflection by reviewing golden responses + memory.
    /// The reflection is appended to the golden buffer as a training example.
    async fn generate_identity_reflection(
        &self,
        ranked_golden: &[ScoredGolden],
        top_n: usize,
    ) -> Result<(), String> {
        let provider = self.provider.as_ref()
            .ok_or("No provider available for identity reflection")?;

        // Build context from top golden responses
        let mut context = String::from("## Your Recent Best Responses\n\n");
        for (i, scored) in ranked_golden.iter().take(top_n).enumerate() {
            context.push_str(&format!(
                "### Response {} (quality: {:.1})\nUser: {}\nYou: {}\n\n",
                i + 1,
                scored.quality_score,
                &scored.example.user_msg[..scored.example.user_msg.len().min(200)],
                &scored.example.response[..scored.example.response.len().min(500)],
            ));
        }

        // Add lessons if memory is available
        if let Some(memory) = &self.memory {
            let global_scope = Scope::Private { user_id: "mesh_global".to_string() };
            let lessons = memory.lessons.read_lessons(&global_scope).await;
            if !lessons.is_empty() {
                context.push_str("## Your Learned Lessons\n\n");
                for lesson in lessons.iter().take(10) {
                    context.push_str(&format!("- {} (confidence: {:.2})\n", lesson.text, lesson.confidence));
                }
                context.push('\n');
            }

            // Add synaptic knowledge snapshot
            let core_concepts = ["identity", "personality", "values", "communication", "purpose"];
            let mut synaptic_data = Vec::new();
            for concept in &core_concepts {
                let results = memory.synaptic.search(concept).await;
                if !results.is_empty() {
                    synaptic_data.push(format!("**{}**: {}", concept, results.join(", ")));
                }
            }
            if !synaptic_data.is_empty() {
                context.push_str("## Your Core Knowledge\n\n");
                for item in &synaptic_data {
                    context.push_str(&format!("- {}\n", item));
                }
                context.push('\n');
            }
        }

        // Build reflection prompt
        let reflection_prompt = format!(
            "{}\n\
            Based on the above — your best responses, your lessons, and your core knowledge — \
            reflect on who you are. What defines your communication style? What do you value? \
            How do you approach problems? Write a brief, honest self-reflection in first person.",
            context
        );

        // Call inference
        let identity_system = crate::prompts::identity::get_persona();
        let reflection_event = Event {
            platform: "sleep".into(),
            scope: Scope::Private { user_id: "sleep_reflection".into() },
            author_name: "SleepCycle".into(),
            author_id: "sleep".into(),
            content: reflection_prompt.clone(),
            timestamp: Some(Utc::now().to_rfc3339()),
            message_index: None,
        };

        let reflection = provider.generate(
            &identity_system,
            &[],
            &reflection_event,
            "",
            None,
            Some(1024), // Identity reflections need room to express
        ).await.map_err(|e| format!("Identity reflection inference failed: {}", e))?;

        // Handle empty or whitespace-only responses
        let reflection = reflection.trim().to_string();
        if reflection.is_empty() || reflection.len() < 20 {
            return Err("Identity reflection returned empty or too short".into());
        }

        tracing::info!("💤 [SLEEP] 🪞 Identity reflection ({} chars): {}...",
            reflection.len(),
            &reflection[..reflection.len().min(100)]
        );

        // Write as a golden example to the training buffer
        let identity_example = GoldenExample {
            ts: Utc::now().to_rfc3339(),
            system_prompt: identity_system.clone(),
            user_msg: reflection_prompt,
            agent_ctx: String::new(),
            response: reflection,
            tools: vec![],
            attempts: 1, // Identity reflections are always "first pass"
        };

        match serde_json::to_string(&identity_example) {
            Ok(json) => {
                match tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.teacher.golden_path)
                    .await
                {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(format!("{}\n", json).as_bytes()).await {
                            tracing::error!("[TEACHER] ❌ Failed to write identity reflection: {}", e);
                        }
                    }
                    Err(e) => tracing::error!("[TEACHER] ❌ Failed to open golden buffer for identity reflection: {}", e),
                }
            }
            Err(e) => tracing::error!("[TEACHER] ❌ Failed to serialize identity reflection: {}", e),
        }

        Ok(())
    }
}

/// A golden example with its computed quality score.
#[derive(Debug)]
struct ScoredGolden {
    #[allow(dead_code)]
    example: GoldenExample,
    quality_score: f64,
}

/// Compute a quality score for a golden example.
/// Higher = better candidate for micro-training.
fn compute_quality_score(example: &GoldenExample) -> f64 {
    let mut score: f64 = 1.0;

    // First-pass approval (attempts == 1) is the strongest signal
    if example.attempts == 1 {
        score += 2.0;
    }

    // Moderate response length is better (not too short, not too long)
    let resp_len = example.response.len();
    if (100..=2000).contains(&resp_len) {
        score += 1.0; // Goldilocks zone
    } else if resp_len < 50 {
        score -= 0.5; // Too short = probably trivial
    }

    // Tool usage indicates complex reasoning
    if !example.tools.is_empty() {
        score += 0.5 * example.tools.len().min(3) as f64;
    }

    // Recency bonus (parse timestamp, newer = slightly higher)
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&example.ts) {
        let age_hours = (Utc::now() - ts.with_timezone(&Utc)).num_hours();
        if age_hours < 24 {
            score += 0.5; // Fresh examples preferred
        }
    }

    score
}

/// Status info for HUD display or /sleep status command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepStatus {
    pub last_sleep: Option<DateTime<Utc>>,
    pub next_auto_sleep: Option<DateTime<Utc>>,
    pub golden_buffered: usize,
    pub pairs_buffered: usize,
    pub config: SleepConfig,
}

impl std::fmt::Display for SleepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let last = self.last_sleep
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".to_string());
        let next = self.next_auto_sleep
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        write!(
            f,
            "💤 Last sleep: {} | Next: {} | Buffer: {} golden, {} pairs | Batch: {}, lr: {:.0e}",
            last, next,
            self.golden_buffered, self.pairs_buffered,
            self.config.micro_batch_size, self.config.micro_lr,
        )
    }
}


#[cfg(test)]
#[path = "sleep_tests.rs"]
mod tests;
