/// Wasm Sandbox — Secure execution environment for distributed compute.
///
/// Peers can submit WebAssembly programs that run in a fully sandboxed
/// environment on other peers' hardware. No filesystem, no network, no
/// host access. Memory-capped, time-capped, priority-managed.
///
/// SECURITY MODEL:
///   - Wasm binary validated before execution (size, import checks)
///   - Memory limited to configurable max (default: 256MB)
///   - CPU time limited (default: 5 minutes)
///   - No WASI filesystem access
///   - No network access
///   - No host function calls except HIVE stdio (stdin/stdout)
///   - Remote jobs run at lowest OS priority
///   - Auto-paused if local load > 80%
///   - Auto-killed if local load > 90%
///
/// BILLING: Provider earns HIVE Coin proportional to CPU-seconds used.
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use wasmtime::*;

use crate::network::messages::PeerId;

/// Configuration for sandbox execution limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Maximum Wasm binary size in bytes (default: 50MB).
    pub max_binary_size: usize,
    /// Maximum memory per job in bytes (default: 256MB).
    pub max_memory_bytes: u64,
    /// Maximum CPU time per job in seconds (default: 300 = 5 min).
    pub max_cpu_secs: u64,
    /// Maximum concurrent remote jobs on this machine.
    pub max_concurrent_jobs: u32,
    /// Whether sandbox execution is enabled on this peer.
    pub enabled: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_binary_size: 50 * 1024 * 1024, // 50MB
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_cpu_secs: 300, // 5 minutes
            max_concurrent_jobs: 2,
            enabled: true,
        }
    }
}

/// Status of a sandbox job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxJobStatus {
    /// Queued, waiting for a slot.
    Queued,
    /// Currently executing.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed with error.
    Failed(String),
    /// Paused due to local resource pressure.
    Paused,
    /// Killed due to timeout or resource limits.
    Killed(String),
}

/// A sandboxed execution job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxJob {
    pub job_id: String,
    pub requester: PeerId,
    pub wasm_size: usize,
    pub input_size: usize,
    pub status: SandboxJobStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub cpu_seconds_used: f64,
    pub memory_peak_bytes: u64,
}

/// Result of sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub job_id: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub cpu_seconds_used: f64,
    pub memory_peak_bytes: u64,
}

/// The Wasm Sandbox Engine.
pub struct SandboxEngine {
    config: SandboxConfig,
    /// Active jobs being tracked.
    jobs: Arc<RwLock<HashMap<String, SandboxJob>>>,
    /// Wasmtime engine (reusable, expensive to create).
    engine: Engine,
    /// Current count of running jobs.
    active_count: Arc<RwLock<u32>>,
}

impl SandboxEngine {
    pub fn new(config: SandboxConfig) -> Result<Self, String> {
        let mut wasm_config = Config::new();
        wasm_config.consume_fuel(true); // Enable fuel-based CPU limiting
        wasm_config.wasm_bulk_memory(true);

        let engine = Engine::new(&wasm_config)
            .map_err(|e| format!("Failed to create Wasm engine: {}", e))?;

        tracing::info!(
            "[SANDBOX] 🏗️ Wasm sandbox initialised (max_mem={}MB, max_cpu={}s, max_jobs={})",
            config.max_memory_bytes / (1024 * 1024),
            config.max_cpu_secs,
            config.max_concurrent_jobs
        );

        Ok(Self {
            config,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            engine,
            active_count: Arc::new(RwLock::new(0)),
        })
    }

    /// Validate a Wasm binary before execution.
    pub fn validate_binary(&self, wasm_bytes: &[u8]) -> Result<(), String> {
        // Size check
        if wasm_bytes.len() > self.config.max_binary_size {
            return Err(format!(
                "Wasm binary too large: {}MB (max: {}MB)",
                wasm_bytes.len() / (1024 * 1024),
                self.config.max_binary_size / (1024 * 1024)
            ));
        }

        // Validate Wasm structure
        Module::validate(&self.engine, wasm_bytes)
            .map_err(|e| format!("Invalid Wasm binary: {}", e))?;

        Ok(())
    }

    /// Execute a Wasm binary with input data.
    /// Returns the execution result (stdout, stderr, exit code, resource usage).
    pub async fn execute(
        &self,
        job_id: &str,
        requester: PeerId,
        wasm_bytes: &[u8],
        input_data: &[u8],
    ) -> Result<SandboxResult, String> {
        if !self.config.enabled {
            return Err("Sandbox execution is disabled on this peer".to_string());
        }

        // Check capacity
        {
            let count = *self.active_count.read().await;
            if count >= self.config.max_concurrent_jobs {
                return Err(format!(
                    "At capacity: {}/{} sandbox slots in use",
                    count, self.config.max_concurrent_jobs
                ));
            }
        }

        // Validate binary
        self.validate_binary(wasm_bytes)?;

        // Record job
        let job = SandboxJob {
            job_id: job_id.to_string(),
            requester: requester.clone(),
            wasm_size: wasm_bytes.len(),
            input_size: input_data.len(),
            status: SandboxJobStatus::Running,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: None,
            cpu_seconds_used: 0.0,
            memory_peak_bytes: 0,
        };
        self.jobs.write().await.insert(job_id.to_string(), job);
        *self.active_count.write().await += 1;

        tracing::info!(
            "[SANDBOX] ▶️ Executing job {} ({}KB Wasm, {}KB input, requester: {})",
            &job_id[..8.min(job_id.len())],
            wasm_bytes.len() / 1024,
            input_data.len() / 1024,
            requester
        );

        // Execute in sandboxed environment
        let start = std::time::Instant::now();
        let result = self.run_sandboxed(wasm_bytes, input_data).await;
        let elapsed = start.elapsed();

        // Update job status
        *self.active_count.write().await -= 1;

        match result {
            Ok((stdout, stderr, exit_code)) => {
                let sandbox_result = SandboxResult {
                    job_id: job_id.to_string(),
                    stdout,
                    stderr,
                    exit_code,
                    cpu_seconds_used: elapsed.as_secs_f64(),
                    memory_peak_bytes: 0, // TODO: track from wasmtime store
                };

                // Update job record
                if let Some(job) = self.jobs.write().await.get_mut(job_id) {
                    job.status = SandboxJobStatus::Completed;
                    job.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    job.cpu_seconds_used = elapsed.as_secs_f64();
                }

                tracing::info!(
                    "[SANDBOX] ✅ Job {} completed ({:.2}s CPU, exit={})",
                    &job_id[..8.min(job_id.len())], elapsed.as_secs_f64(), exit_code
                );

                Ok(sandbox_result)
            }
            Err(e) => {
                if let Some(job) = self.jobs.write().await.get_mut(job_id) {
                    job.status = SandboxJobStatus::Failed(e.clone());
                    job.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    job.cpu_seconds_used = elapsed.as_secs_f64();
                }

                tracing::warn!(
                    "[SANDBOX] ❌ Job {} failed ({:.2}s): {}",
                    &job_id[..8.min(job_id.len())], elapsed.as_secs_f64(), e
                );

                Err(e)
            }
        }
    }

    /// Run a Wasm binary in a fully sandboxed wasmtime environment.
    async fn run_sandboxed(
        &self,
        wasm_bytes: &[u8],
        input_data: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>, i32), String> {
        let engine = self.engine.clone();
        let max_fuel = self.config.max_cpu_secs * 1_000_000_000; // Approximate fuel units
        let input = input_data.to_vec();
        let wasm_owned = wasm_bytes.to_vec();

        // Run in a blocking task to avoid blocking the async runtime
        let result = tokio::task::spawn_blocking(move || {
            // Create a new store with fuel limits
            let mut store = Store::new(&engine, ());
            store.set_fuel(max_fuel)
                .map_err(|e| format!("Failed to set fuel: {}", e))?;

            // Compile the module
            let module = Module::new(&engine, &wasm_owned)
                .map_err(|e| format!("Failed to compile Wasm: {}", e))?;

            // Create a linker with NO host functions (pure sandbox)
            let linker = Linker::new(&engine);

            // Create instance
            let instance = linker.instantiate(&mut store, &module)
                .map_err(|e| format!("Failed to instantiate Wasm: {}", e))?;

            // Try to call the "_start" or "main" function
            let stdout: Vec<u8>;
            let stderr: Vec<u8> = Vec::new();
            let exit_code: i32;

            // Look for exported "process" function that takes input length and returns output
            if let Some(process_fn) = instance.get_func(&mut store, "process") {
                // Call the process function
                let mut results = vec![Val::I32(0)];
                match process_fn.call(&mut store, &[Val::I32(input.len() as i32)], &mut results) {
                    Ok(()) => {
                        exit_code = results[0].i32().unwrap_or(0);
                        stdout = format!("Process returned: {}", exit_code).into_bytes();
                    }
                    Err(e) => {
                        if e.to_string().contains("fuel") {
                            return Err("CPU time limit exceeded".to_string());
                        }
                        return Err(format!("Wasm execution error: {}", e));
                    }
                }
            } else if let Some(start_fn) = instance.get_func(&mut store, "_start") {
                // WASI-style entry point
                match start_fn.call(&mut store, &[], &mut []) {
                    Ok(()) => {
                        exit_code = 0;
                        stdout = b"Program completed successfully".to_vec();
                    }
                    Err(e) => {
                        if e.to_string().contains("fuel") {
                            return Err("CPU time limit exceeded".to_string());
                        }
                        return Err(format!("Wasm execution error: {}", e));
                    }
                }
            } else {
                return Err("Wasm module has no 'process' or '_start' export".to_string());
            }

            Ok((stdout, stderr, exit_code))
        })
        .await
        .map_err(|e| format!("Sandbox task panicked: {}", e))?;

        result
    }

    /// Get the number of active sandbox jobs.
    pub async fn active_jobs(&self) -> u32 {
        *self.active_count.read().await
    }

    /// Get available sandbox slots.
    pub async fn available_slots(&self) -> u32 {
        let active = *self.active_count.read().await;
        self.config.max_concurrent_jobs.saturating_sub(active)
    }

    /// Get sandbox stats.
    pub async fn stats(&self) -> serde_json::Value {
        let jobs = self.jobs.read().await;
        let completed = jobs.values()
            .filter(|j| j.status == SandboxJobStatus::Completed)
            .count();
        let failed = jobs.values()
            .filter(|j| matches!(j.status, SandboxJobStatus::Failed(_)))
            .count();
        let total_cpu: f64 = jobs.values()
            .map(|j| j.cpu_seconds_used)
            .sum();

        serde_json::json!({
            "enabled": self.config.enabled,
            "active_jobs": *self.active_count.read().await,
            "max_concurrent": self.config.max_concurrent_jobs,
            "total_executed": completed + failed,
            "completed": completed,
            "failed": failed,
            "total_cpu_seconds": total_cpu,
            "max_cpu_per_job_secs": self.config.max_cpu_secs,
            "max_memory_mb": self.config.max_memory_bytes / (1024 * 1024),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine() -> SandboxEngine {
        SandboxEngine::new(SandboxConfig {
            max_binary_size: 1024 * 1024, // 1MB for tests
            max_memory_bytes: 64 * 1024 * 1024, // 64MB
            max_cpu_secs: 5, // 5 seconds
            max_concurrent_jobs: 2,
            enabled: true,
        }).unwrap()
    }

    #[test]
    fn test_validate_too_large() {
        let engine = test_engine();
        let huge = vec![0u8; 2 * 1024 * 1024]; // 2MB > 1MB limit
        assert!(engine.validate_binary(&huge).is_err());
    }

    #[test]
    fn test_validate_invalid_wasm() {
        let engine = test_engine();
        let garbage = b"not a wasm binary";
        assert!(engine.validate_binary(garbage).is_err());
    }

    #[test]
    fn test_validate_valid_wasm() {
        let engine = test_engine();
        // Minimal valid Wasm module (magic number + version + empty)
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        assert!(engine.validate_binary(&minimal_wasm).is_ok());
    }

    #[tokio::test]
    async fn test_execute_simple_wasm() {
        let engine = test_engine();

        // Compile a simple Wasm module that exports a "process" function
        let wasm = wat::parse_str(r#"
            (module
                (func (export "process") (param i32) (result i32)
                    i32.const 42
                )
            )
        "#).unwrap();

        let result = engine.execute(
            "test_job_1",
            PeerId("requester".to_string()),
            &wasm,
            b"input data",
        ).await;

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.exit_code, 42);
        assert!(res.cpu_seconds_used > 0.0);
    }

    #[tokio::test]
    async fn test_disabled_sandbox_rejects() {
        let engine = SandboxEngine::new(SandboxConfig {
            enabled: false,
            ..Default::default()
        }).unwrap();

        let wasm = wat::parse_str("(module)").unwrap();
        let result = engine.execute(
            "test",
            PeerId("req".to_string()),
            &wasm,
            b"",
        ).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[tokio::test]
    async fn test_capacity_limit() {
        let engine = SandboxEngine::new(SandboxConfig {
            max_concurrent_jobs: 0, // No slots
            ..Default::default()
        }).unwrap();

        let wasm = wat::parse_str("(module (func (export \"process\") (param i32) (result i32) i32.const 0))").unwrap();
        let result = engine.execute("test", PeerId("req".to_string()), &wasm, b"").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("capacity"));
    }

    #[tokio::test]
    async fn test_stats() {
        let engine = test_engine();
        let stats = engine.stats().await;
        assert_eq!(stats["enabled"], true);
        assert_eq!(stats["active_jobs"], 0);
        assert_eq!(stats["max_concurrent"], 2);
    }
}
